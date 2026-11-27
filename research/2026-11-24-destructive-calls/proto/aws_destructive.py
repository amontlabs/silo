#!/usr/bin/env python3
"""Derive AWS destructive operations, and how each looks on the wire, from botocore's service models.

Usage:
    pip download botocore --no-deps -d /some/empty/dir
    unzip -q /some/empty/dir/botocore-*.whl 'botocore/data/*' -d /some/empty/dir/x
    python3 -I aws_destructive.py /some/empty/dir/x/botocore/data > aws_destructive.json

The data directory holds one folder per service, each with one folder per API version, each with
service-2.json or service-2.json.gz. Only the newest API version of each service is read.

What "destructive" means here: the operation name starts with one of VERBS, or the operation is in
EXTRA_OPS (names that do not start with such a verb but delete, stop or expose something). This is a
name filter: it over-counts (DeleteTags, CancelExportTask) and the `top` list and the
`destructive_params` list are the hand-checked part. Every name in them is asserted to exist in the
models, so a rename upstream fails this script instead of silently matching nothing.

How each protocol shows the operation (from botocore/serialize.py, read 2026-11-24, botocore 1.44.10):
  query / ec2         POST https://<host>/  form body Action=<Op>&Version=<apiVersion>&...
                      (GET with the same pairs in the query string is also accepted by AWS)
  json                POST https://<host>/  header X-Amz-Target: <targetPrefix>.<Op>
                      Content-Type application/x-amz-json-1.0 or 1.1
  rest-json/rest-xml  <method> <requestUri template>  (S3: bucket may be in the host instead)
  smithy-rpc-v2-cbor  POST https://<host>/service/<targetPrefix>/operation/<Op>
                      header smithy-protocol: rpc-v2-cbor, body CBOR
A service listing several protocols in metadata.protocols may be reached through any of them,
depending on the SDK and its version. botocore picks the first supported one in the order
json, rest-json, rest-xml, smithy-rpc-v2-cbor, query, ec2 (botocore/utils.py
PRIORITY_ORDERED_SUPPORTED_PROTOCOLS); other SDKs may pick CBOR. So a matcher must accept all.
"""
import gzip
import json
import os
import re
import sys

VERBS = ("Delete", "Terminate", "Remove", "Purge", "Drop", "Destroy", "Disable", "Revoke",
         "Detach", "Deregister", "Cancel", "Reset", "Restore")

# Destructive operations whose names do not start with one of VERBS.
EXTRA_OPS = {
    "kms": ["ScheduleKeyDeletion"],
    "ec2": ["ReleaseAddress", "StopInstances", "DisassociateAddress"],
    "organizations": ["CloseAccount", "LeaveOrganization"],
    "dynamodb": ["BatchWriteItem"],
    "s3": ["PutBucketLifecycleConfiguration", "PutBucketVersioning", "PutBucketPolicy"],
    "route53": ["ChangeResourceRecordSets"],
    "lambda": ["PutFunctionConcurrency"],
    "rds": ["StopDBInstance", "StopDBCluster"],
}

# The operations that matter most for coding agents, with the outcome word from the brief.
TOP = [
    ("s3", "DeleteBucket", "outage", "Only succeeds on an empty bucket. The data loss is the DeleteObjects calls before it."),
    ("s3", "DeleteObjects", "gone", "Up to 1000 keys per call. Recoverable only if versioning was on (it adds delete markers) and no VersionId is named."),
    ("s3", "DeleteObject", "gone", "Same as DeleteObjects for one key. With VersionId it removes that version for good."),
    ("s3", "PutBucketLifecycleConfiguration", "gone", "An Expiration or NoncurrentVersionExpiration rule deletes data later, asynchronously."),
    ("s3", "PutBucketVersioning", "gone", "Status=Suspended in the XML body: later overwrites and deletes are no longer kept."),
    ("rds", "DeleteDBInstance", "gone", "SkipFinalSnapshot=true and DeleteAutomatedBackups=true (the default) leave nothing. Refused while DeletionProtection is on."),
    ("rds", "DeleteDBCluster", "gone", "Same parameters as DeleteDBInstance for Aurora and Multi-AZ clusters."),
    ("rds", "ModifyDBInstance", "gone", "DeletionProtection=false or BackupRetentionPeriod=0 prepares a later loss."),
    ("rds", "DeleteDBSnapshot", "gone", "Manual snapshot removed."),
    ("rds", "DeleteDBClusterSnapshot", "gone", "Manual cluster snapshot removed."),
    ("dynamodb", "DeleteTable", "gone", "Refused while DeletionProtectionEnabled. AWS keeps PITR for 35 days only if PITR was on."),
    ("dynamodb", "UpdateTable", "gone", "DeletionProtectionEnabled=false prepares DeleteTable."),
    ("dynamodb", "UpdateContinuousBackups", "gone", "PointInTimeRecoveryEnabled=false removes the restore window."),
    ("dynamodb", "BatchWriteItem", "gone", "DeleteRequest entries inside RequestItems delete items."),
    ("dynamodb", "DeleteBackup", "gone", "On-demand backup removed."),
    ("ec2", "TerminateInstances", "gone", "Root and other EBS volumes with DeleteOnTermination=true go with it. Refused while DisableApiTermination is set."),
    ("ec2", "DeleteVolume", "gone", "EBS volume. Recoverable only if an EC2 Recycle Bin retention rule matches volumes."),
    ("ec2", "DeleteSnapshot", "recoverable", "Recoverable only if a Recycle Bin retention rule matches the snapshot."),
    ("ec2", "ModifyInstanceAttribute", "gone", "DisableApiTermination=false prepares TerminateInstances."),
    ("lambda", "DeleteFunction", "outage", "Code can be redeployed from source. Versions and aliases go too."),
    ("lambda", "PutFunctionConcurrency", "outage", "ReservedConcurrentExecutions=0 stops all invocations."),
    ("ecs", "DeleteService", "outage", "force=true deletes a service that still has tasks."),
    ("ecs", "UpdateService", "outage", "desiredCount=0 stops all tasks."),
    ("ecs", "DeleteCluster", "outage", ""),
    ("eks", "DeleteCluster", "outage", "Control plane, and with it etcd state (all Kubernetes objects). Refused while deletionProtection is on (added Aug 2025)."),
    ("eks", "UpdateClusterConfig", "outage", "deletionProtection=false prepares DeleteCluster."),
    ("cloudformation", "DeleteStack", "gone", "Deletes every resource of the stack whose DeletionPolicy is not Retain/Snapshot. DeletionMode=FORCE_DELETE_STACK."),
    ("cloudformation", "UpdateStack", "gone", "A template that drops or replaces a resource deletes it."),
    ("cloudformation", "UpdateTerminationProtection", "gone", "EnableTerminationProtection=false prepares DeleteStack."),
    ("route53", "ChangeResourceRecordSets", "outage", "<Action>DELETE</Action> or UPSERT inside the XML ChangeBatch."),
    ("route53", "DeleteHostedZone", "outage", "Zone gone; recreating it gives new name servers."),
    ("iam", "DeleteRole", "outage", ""),
    ("iam", "DeleteUser", "outage", ""),
    ("iam", "DeleteAccessKey", "outage", ""),
    ("kms", "ScheduleKeyDeletion", "recoverable", "7 to 30 days pending (default 30), CancelKeyDeletion undoes it. After that, data encrypted under the key is gone."),
    ("kms", "DisableKey", "outage", "Everything encrypted with the key becomes unreadable until EnableKey."),
    ("secretsmanager", "DeleteSecret", "recoverable", "7 to 30 day recovery window (default 30), RestoreSecret undoes it. ForceDeleteWithoutRecovery=true makes it gone."),
    ("ecr", "DeleteRepository", "gone", "force=true deletes a repository that still holds images."),
    ("ecr", "BatchDeleteImage", "gone", ""),
    ("elasticache", "DeleteReplicationGroup", "gone", "FinalSnapshotIdentifier absent means no final snapshot."),
    ("elasticache", "DeleteCacheCluster", "gone", ""),
    ("cloudfront", "DeleteDistribution", "outage", "Only a disabled distribution can be deleted, so UpdateDistribution (Enabled=false) comes first."),
    ("efs", "DeleteFileSystem", "gone", ""),
    ("redshift", "DeleteCluster", "gone", "SkipFinalClusterSnapshot=true."),
    ("logs", "DeleteLogGroup", "gone", ""),
    ("backup", "DeleteRecoveryPoint", "gone", "Refused by a vault lock until the minimum retention passes."),
    ("backup", "DeleteBackupVault", "gone", "Only an empty vault."),
    ("organizations", "CloseAccount", "recoverable", "90-day post-closure period in which AWS Support can reopen the account."),
    ("ce", "DeleteCostCategoryDefinition", "outage", "Cost Explorer, named because the Kiro incident hit that service."),
    ("ce", "DeleteAnomalyMonitor", "outage", ""),
]

# Non-delete calls where one parameter makes them destructive.
DESTRUCTIVE_PARAMS = [
    ("rds", "ModifyDBInstance", "DeletionProtection", "false"),
    ("rds", "ModifyDBInstance", "BackupRetentionPeriod", "0 (turns automated backups off and deletes them)"),
    ("rds", "ModifyDBCluster", "DeletionProtection", "false"),
    ("rds", "DeleteDBInstance", "SkipFinalSnapshot", "true"),
    ("rds", "DeleteDBInstance", "DeleteAutomatedBackups", "true, or absent (default true)"),
    ("rds", "DeleteDBCluster", "SkipFinalSnapshot", "true"),
    ("rds", "DeleteDBCluster", "DeleteAutomatedBackups", "true, or absent (default true)"),
    ("dynamodb", "UpdateTable", "DeletionProtectionEnabled", "false"),
    ("dynamodb", "UpdateContinuousBackups", "PointInTimeRecoverySpecification", "PointInTimeRecoveryEnabled=false"),
    ("dynamodb", "BatchWriteItem", "RequestItems", "any DeleteRequest entry"),
    ("ec2", "ModifyInstanceAttribute", "DisableApiTermination", "false"),
    ("ec2", "TerminateInstances", "Force", "true"),
    ("s3", "PutBucketVersioning", "VersioningConfiguration", "<Status>Suspended</Status>"),
    ("s3", "PutBucketLifecycleConfiguration", "LifecycleConfiguration", "<Expiration> or <NoncurrentVersionExpiration>"),
    ("s3", "DeleteObject", "VersionId", "present (permanent delete of that version)"),
    ("s3", "DeleteObjects", "Delete", "<VersionId> inside an <Object> (permanent)"),
    ("s3", "DeleteObject", "BypassGovernanceRetention", "true"),
    ("s3", "PutBucketPolicy", "Policy", "a Deny on the caller's own access (ConfirmRemoveSelfBucketAccess)"),
    ("secretsmanager", "DeleteSecret", "ForceDeleteWithoutRecovery", "true"),
    ("kms", "ScheduleKeyDeletion", "PendingWindowInDays", "7 (the minimum)"),
    ("cloudformation", "UpdateTerminationProtection", "EnableTerminationProtection", "false"),
    ("cloudformation", "DeleteStack", "DeletionMode", "FORCE_DELETE_STACK"),
    ("cloudformation", "UpdateStack", "TemplateBody", "a template that removes or replaces resources (needs a change set or plan to know)"),
    ("ecs", "UpdateService", "desiredCount", "0"),
    ("ecs", "DeleteService", "force", "true"),
    ("ecr", "DeleteRepository", "force", "true"),
    ("lambda", "PutFunctionConcurrency", "ReservedConcurrentExecutions", "0"),
    ("autoscaling", "UpdateAutoScalingGroup", "DesiredCapacity", "0"),
    ("autoscaling", "UpdateAutoScalingGroup", "DeletionProtection", "none"),
    ("route53", "ChangeResourceRecordSets", "ChangeBatch", "<Action>DELETE</Action>"),
    ("redshift", "DeleteCluster", "SkipFinalClusterSnapshot", "true"),
    ("eks", "UpdateClusterConfig", "deletionProtection", "false"),
]


def load_models(data_dir):
    models = {}
    for svc in sorted(os.listdir(data_dir)):
        sdir = os.path.join(data_dir, svc)
        if not os.path.isdir(sdir):
            continue
        versions = sorted(v for v in os.listdir(sdir) if os.path.isdir(os.path.join(sdir, v)))
        for v in reversed(versions):
            for name in ("service-2.json.gz", "service-2.json"):
                p = os.path.join(sdir, v, name)
                if os.path.exists(p):
                    opener = gzip.open if p.endswith(".gz") else open
                    with opener(p, "rt", encoding="utf-8") as f:
                        models[svc] = json.load(f)
                    break
            if svc in models:
                break
    return models


def path_regex(template):
    """/2015-03-31/functions/{FunctionName} -> ^/2015-03-31/functions/[^/]+$ ; {Key+} matches slashes."""
    path, _, query = template.partition("?")
    out = ""
    for part in re.split(r"(\{[^}]+\})", path):
        if part.startswith("{"):
            out += ".+" if part.endswith("+}") else "[^/]+"
        else:
            out += re.escape(part)
    return "^" + out + "/?$", query  # query is the required query key (e.g. "delete" for S3 DeleteObjects)


def wire_name(model, protocol, op, member):
    """Name under which an input member appears on the wire for this protocol."""
    shape = model["shapes"][model["operations"][op]["input"]["shape"]]
    m = shape["members"][member]
    if protocol == "ec2":
        return m.get("queryName") or (m.get("locationName", member)[:1].upper() + m.get("locationName", member)[1:])
    if protocol in ("rest-json", "rest-xml") and m.get("location"):
        return f'{m["location"]}:{m.get("locationName", member)}'
    return m.get("locationName", member)


def match_for(model, op_name):
    md = model["metadata"]
    op = model["operations"][op_name]
    protos = md.get("protocols") or [md["protocol"]]
    out = {}
    for p in protos:
        if p in ("query", "ec2"):
            out[p] = {"method": "POST", "path": "/", "form": {"Action": op_name, "Version": md["apiVersion"]}}
        elif p == "json":
            out[p] = {"method": "POST", "path": "/",
                      "header": {"X-Amz-Target": f'{md["targetPrefix"]}.{op_name}'},
                      "content_type": f'application/x-amz-json-{md.get("jsonVersion", "1.0")}'}
        elif p == "smithy-rpc-v2-cbor":
            out[p] = {"method": "POST", "path": f'/service/{md["targetPrefix"]}/operation/{op_name}',
                      "header": {"smithy-protocol": "rpc-v2-cbor"}}
        elif p in ("rest-json", "rest-xml"):
            rx, q = path_regex(op["http"]["requestUri"])
            out[p] = {"method": op["http"]["method"], "path_template": op["http"]["requestUri"],
                      "path_regex": rx, "required_query_key": q or None}
            if md.get("endpointPrefix") == "s3" and "{Bucket}" in op["http"]["requestUri"]:
                rest = op["http"]["requestUri"].replace("/{Bucket}", "", 1)
                rest = rest if rest.startswith("/") else "/" + rest
                out[p]["virtual_hosted"] = {"host": "<bucket>.s3.<region>.amazonaws.com", "path_template": rest}
    return out


def main(data_dir):
    models = load_models(data_dir)
    for svc, op, *_ in TOP + [(s, o, None) for s, o, _, _ in DESTRUCTIVE_PARAMS]:
        assert svc in models and op in models[svc]["operations"], f"missing {svc}.{op}"
    for svc, op, param, _ in DESTRUCTIVE_PARAMS:
        shape = models[svc]["shapes"][models[svc]["operations"][op]["input"]["shape"]]
        assert param in shape["members"], f"missing {svc}.{op}.{param}"
    top = {(s, o): (outcome, note) for s, o, outcome, note in TOP}

    ops, by_verb = [], {}
    for svc, model in models.items():
        md = model["metadata"]
        for name in sorted(model["operations"]):
            verb = next((v for v in VERBS if name.startswith(v)), None)
            if verb is None and name not in EXTRA_OPS.get(svc, []) and (svc, name) not in top:
                continue
            by_verb[verb or "other"] = by_verb.get(verb or "other", 0) + 1
            entry = {
                "service": svc,
                "endpoint_prefix": md.get("endpointPrefix"),
                "signing_name": md.get("signingName", md.get("endpointPrefix")),
                "api_version": md["apiVersion"],
                "protocols": md.get("protocols") or [md["protocol"]],
                "operation": name,
                "match": match_for(model, name),
            }
            if (svc, name) in top:
                entry["top"] = True
                entry["outcome"], entry["note"] = top[(svc, name)]
            ops.append(entry)

    params = []
    for svc, op, param, value in DESTRUCTIVE_PARAMS:
        md = models[svc]["metadata"]
        p = (md.get("protocols") or [md["protocol"]])[0]
        params.append({"service": svc, "operation": op, "parameter": param,
                       "wire_name": wire_name(models[svc], p, op, param), "protocol": p,
                       "dangerous_value": value})

    proto_count = {}
    for m in models.values():
        k = "+".join(m["metadata"].get("protocols") or [m["metadata"]["protocol"]])
        proto_count[k] = proto_count.get(k, 0) + 1

    json.dump({
        "source": "botocore service-2 models (newest API version per service)",
        "services": len(models),
        "operations_total": sum(len(m["operations"]) for m in models.values()),
        "services_by_protocols": dict(sorted(proto_count.items(), key=lambda x: -x[1])),
        "destructive_by_verb": dict(sorted(by_verb.items(), key=lambda x: -x[1])),
        "destructive_total": len(ops),
        "top_count": len(TOP),
        "destructive_params": params,
        "operations": sorted(ops, key=lambda e: (not e.get("top", False), e["service"], e["operation"])),
    }, sys.stdout, indent=1)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main(sys.argv[1])
