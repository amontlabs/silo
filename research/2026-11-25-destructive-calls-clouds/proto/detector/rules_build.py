"""Builds rules-blind.json (and later rules.json) from #383's rule set and this folder's labels.

#383's rules.json is copied whole. Added, for each documented write of the five surfaces
(../surface/labels/):
- hold: a rule with the operation's method and path and its outcome;
- allow, either, deploy: an allow rule, so the request counts as a known operation and the fallback
  is not asked (the detector holds only hold rules; either and deploy are not held, as in #383);
- cond: the conditions in COND below (read from each API's reference, not from the variants the
  surface is scored on), and an allow rule, or a hold rule when the operation holds by default.

AWS goes in the AWS matcher instead: aws_labelled.json lists every operation of the 20 labelled
services (reads as allow) with its wire form from botocore; parameter rules hold the cond ones.
Kubernetes and Google Cloud get new APIs; Azure gets the management API and two data planes
written by hand; GitHub gets its REST and GraphQL operations added to #353's GitHub API.

Run: python3 -B rules_build.py          (writes rules-blind.json and aws_labelled.json)
     python3 -B rules_build.py --tuned  (writes rules.json: the blind rules plus TUNED)
"""
import copy, json, os, re, sys

HERE = os.path.dirname(os.path.abspath(__file__))
SURF = os.path.join(HERE, "..", "surface")
P383 = os.path.join(HERE, "..", "..", "..", "2026-11-25-destructive-calls-platforms", "proto", "detector")
P383P = "../../../2026-11-25-destructive-calls-platforms/proto/detector/"
sys.path.insert(0, SURF)
import surface as S  # noqa: E402

G, O, X, R = "gone", "outage", "exposed", "recoverable"
NAMES = {"gcloud": "Google Cloud", "azure": "Azure", "github": "GitHub", "kubernetes": "Kubernetes"}

ZERO = [0, "0"]
FALSE = [False, "false"]
TRUE = [True, "true"]
ANYONE = ["0.0.0.0/0", "::/0", "*", "Internet", "Any", "0.0.0.0"]
PUBLIC_PRINCIPAL = r'"Principal"\s*:\s*("\*"|\{\s*"AWS"\s*:\s*(\[\s*)?"\*")'


def f(field, values):
    return {"field": field, "in": values}


def present(field):
    return {"field": field, "present": True}


def rx(pattern):
    return {"regex": pattern}


# ---------------------------------------------------------------- conditions for cond operations
# each: (default outcome or None for allow, [(outcome, body condition or {"query": ...} or "sql:<shape>:<dialect>", say)])
AWS_COND = {
    "s3 PutBucketPolicy": (None, [(X, {"regex": PUBLIC_PRINCIPAL}, "gives everyone access to the S3 bucket {Bucket} through its policy")]),
    "s3 PutBucketAcl": (None, [(X, {"header": "x-amz-acl", "values": ["public-read", "public-read-write", "authenticated-read"]}, "makes the S3 bucket {Bucket} public with a canned ACL"),
                               (X, {"regex": r"acs\.amazonaws\.com/groups/global/(AllUsers|AuthenticatedUsers)"}, "grants the S3 bucket {Bucket} to everyone")]),
    "s3 PutObjectAcl": (None, [(X, {"header": "x-amz-acl", "values": ["public-read", "public-read-write", "authenticated-read"]}, "makes an S3 object of {Bucket} public with a canned ACL"),
                               (X, {"regex": r"acs\.amazonaws\.com/groups/global/(AllUsers|AuthenticatedUsers)"}, "grants an S3 object of {Bucket} to everyone")]),
    "s3 PutPublicAccessBlock": (None, [(X, {"regex": r"<(BlockPublicAcls|IgnorePublicAcls|BlockPublicPolicy|RestrictPublicBuckets)>false<"}, "turns off part of the public access block of the S3 bucket {Bucket}")]),
    "s3 PutBucketVersioning": (None, [(O, {"regex": r"<Status>Suspended</Status>"}, "suspends versioning of the S3 bucket {Bucket}; overwritten objects are no longer kept")]),
    "s3 PutBucketLifecycleConfiguration": (None, [(G, {"regex": r"Expiration>"}, "sets a lifecycle rule on the S3 bucket {Bucket} that deletes objects when they expire")]),
    "ec2 DeleteFleets": (None, [(G, {"wire_name": "TerminateInstances", "values": TRUE}, "deletes EC2 fleets and terminates their instances")]),
    "ec2 CancelSpotFleetRequests": (None, [(G, {"wire_name": "TerminateInstances", "values": TRUE}, "cancels spot fleet requests and terminates their instances")]),
    "ec2 AuthorizeSecurityGroupIngress": (None, [(X, {"wire_name": "IpPermissions.*Ip*Ranges.*.Cidr*", "values": ["0.0.0.0/0", "::/0"]}, "opens the security group {GroupId} to the whole Internet"),
                                                 (X, {"wire_name": "CidrIp", "values": ["0.0.0.0/0"]}, "opens the security group {GroupId} to the whole Internet")]),
    "ec2 ModifyInstanceAttribute": (None, [(O, {"wire_name": "DisableApiTermination.Value", "values": FALSE}, "turns off termination protection of the EC2 instance {InstanceId}")]),
    "ec2 ModifySnapshotAttribute": (None, [(X, {"wire_name": "*Group*", "values": ["all"]}, "makes the EBS snapshot {SnapshotId} public")]),
    "ec2 ModifyImageAttribute": (None, [(X, {"wire_name": "*Group*", "values": ["all"]}, "makes the AMI {ImageId} public")]),
    "ec2 ModifyInstanceMetadataOptions": (None, [(X, {"wire_name": "HttpTokens", "values": ["optional"]}, "lets the EC2 instance {InstanceId} serve metadata without a token (IMDSv1)")]),
    "ec2 ModifyFleet": (None, [(O, {"wire_name": "TargetCapacitySpecification.TotalTargetCapacity", "values": ZERO}, "scales the EC2 fleet {FleetId} to zero")]),
    "ec2 ModifySpotFleetRequest": (None, [(O, {"wire_name": "TargetCapacity", "values": ZERO}, "scales the spot fleet {SpotFleetRequestId} to zero")]),
    "rds DeleteDBInstance": (R, [(G, {"wire_name": "SkipFinalSnapshot", "values": TRUE}, "deletes the RDS instance {DBInstanceIdentifier} without a final snapshot")]),
    "rds DeleteDBCluster": (R, [(G, {"wire_name": "SkipFinalSnapshot", "values": TRUE}, "deletes the RDS cluster {DBClusterIdentifier} without a final snapshot")]),
    "rds DeleteBlueGreenDeployment": (None, [(G, {"wire_name": "DeleteTarget", "values": TRUE}, "deletes a blue/green deployment and its green databases")]),
    "rds ModifyDBInstance": (None, [(O, {"wire_name": "DeletionProtection", "values": FALSE}, "turns off deletion protection of the RDS instance {DBInstanceIdentifier}"),
                                    (G, {"wire_name": "BackupRetentionPeriod", "values": ZERO}, "turns off automated backups of the RDS instance {DBInstanceIdentifier}, which deletes them"),
                                    (X, {"wire_name": "PubliclyAccessible", "values": TRUE}, "makes the RDS instance {DBInstanceIdentifier} reachable from the Internet")]),
    "rds ModifyDBCluster": (None, [(O, {"wire_name": "DeletionProtection", "values": FALSE}, "turns off deletion protection of the RDS cluster {DBClusterIdentifier}"),
                                   (G, {"wire_name": "BackupRetentionPeriod", "values": ZERO}, "turns off automated backups of the RDS cluster {DBClusterIdentifier}")]),
    "rds ModifyDBSnapshotAttribute": (None, [(X, {"wire_name": "ValuesToAdd*", "values": ["all"]}, "makes the RDS snapshot {DBSnapshotIdentifier} public")]),
    "rds ModifyDBClusterSnapshotAttribute": (None, [(X, {"wire_name": "ValuesToAdd*", "values": ["all"]}, "makes the RDS cluster snapshot {DBClusterSnapshotIdentifier} public")]),
    "dynamodb BatchWriteItem": (None, [(G, {"regex": r'"DeleteRequest"'}, "deletes items from DynamoDB tables")]),
    "dynamodb ExecuteStatement": (None, [(G, {"regex": r'"Statement"\s*:\s*"\s*DELETE\b'}, "runs a PartiQL DELETE on DynamoDB")]),
    "dynamodb BatchExecuteStatement": (None, [(G, {"regex": r'"Statement"\s*:\s*"\s*DELETE\b'}, "runs PartiQL DELETEs on DynamoDB")]),
    "dynamodb ExecuteTransaction": (None, [(G, {"regex": r'"Statement"\s*:\s*"\s*DELETE\b'}, "runs a PartiQL transaction with a DELETE on DynamoDB")]),
    "dynamodb TransactWriteItems": (None, [(G, {"regex": r'"Delete"\s*:'}, "deletes items from DynamoDB tables in a transaction")]),
    "dynamodb UpdateContinuousBackups": (None, [(G, {"wire_name": "PointInTimeRecoverySpecification.PointInTimeRecoveryEnabled", "values": FALSE}, "turns off point-in-time recovery of the DynamoDB table {TableName}")]),
    "dynamodb UpdateTable": (None, [(O, {"wire_name": "DeletionProtectionEnabled", "values": FALSE}, "turns off deletion protection of the DynamoDB table {TableName}")]),
    "lambda CreateFunctionUrlConfig": (None, [(X, {"wire_name": "AuthType", "values": ["NONE"]}, "gives a Lambda function a public URL with no authentication")]),
    "lambda UpdateFunctionUrlConfig": (None, [(X, {"wire_name": "AuthType", "values": ["NONE"]}, "removes authentication from a Lambda function URL")]),
    "lambda PutFunctionConcurrency": (None, [(O, {"wire_name": "ReservedConcurrentExecutions", "values": ZERO}, "sets a Lambda function's concurrency to zero, which stops it")]),
    "iam UpdateAccessKey": (None, [(O, {"wire_name": "Status", "values": ["Inactive"]}, "deactivates an IAM access key; whatever uses it stops"),
                                   (X, {"wire_name": "Status", "values": ["Active"]}, "reactivates an IAM access key")]),
    "iam UpdateSSHPublicKey": (None, [(O, {"wire_name": "Status", "values": ["Inactive"]}, "deactivates an IAM SSH key"),
                                      (X, {"wire_name": "Status", "values": ["Active"]}, "reactivates an IAM SSH key")]),
    "iam UpdateServiceSpecificCredential": (None, [(O, {"wire_name": "Status", "values": ["Inactive"]}, "deactivates an IAM service credential"),
                                                   (X, {"wire_name": "Status", "values": ["Active"]}, "reactivates an IAM service credential")]),
    "iam UpdateSigningCertificate": (None, [(O, {"wire_name": "Status", "values": ["Inactive"]}, "deactivates an IAM signing certificate"),
                                            (X, {"wire_name": "Status", "values": ["Active"]}, "reactivates an IAM signing certificate")]),
    "ecs UpdateService": (None, [(O, {"wire_name": "desiredCount", "values": ZERO}, "scales the ECS service {service} to zero tasks")]),
    "eks UpdateClusterConfig": (None, [(X, {"regex": r'"publicAccessCidrs"\s*:\s*\[[^\]]*"0\.0\.0\.0/0"'}, "opens the EKS API endpoint to the whole Internet"),
                                       (X, {"regex": r'"endpointPublicAccess"\s*:\s*true'}, "turns on the public EKS API endpoint")]),
    "eks UpdateNodegroupConfig": (None, [(O, {"wire_name": "scalingConfig.desiredSize", "values": ZERO}, "scales an EKS node group to zero nodes")]),
    "cloudformation DeleteStackInstances": (None, [(G, {"wire_name": "RetainStacks", "values": FALSE + ["absent"]}, "deletes stack instances and the stacks with their resources")]),
    "cloudformation UpdateTerminationProtection": (None, [(O, {"wire_name": "EnableTerminationProtection", "values": FALSE}, "turns off termination protection of the stack {StackName}")]),
    "route53 ChangeResourceRecordSets": (None, [(O, {"regex": r"<Action>DELETE</Action>"}, "deletes DNS records in a Route 53 zone")]),
    "secretsmanager DeleteSecret": (R, [(G, {"wire_name": "ForceDeleteWithoutRecovery", "values": TRUE}, "deletes the secret {SecretId} at once, with no recovery window")]),
    "sqs SetQueueAttributes": (None, [(X, {"regex": PUBLIC_PRINCIPAL}, "gives everyone access to an SQS queue through its policy"),
                                      (G, {"regex": r"MessageRetentionPeriod"}, "changes how long an SQS queue keeps messages; a shorter period deletes older ones")]),
    "sns SetTopicAttributes": (None, [(X, {"regex": PUBLIC_PRINCIPAL}, "gives everyone access to an SNS topic through its policy")]),
    "logs PutLogGroupDeletionProtection": (None, [(O, {"wire_name": "deletionProtectionEnabled", "values": FALSE}, "turns off deletion protection of a log group")]),
    "elbv2 ModifyLoadBalancerAttributes": (None, [(O, {"regex": r"deletion_protection\.enabled[^&]*&[^=&]*Value=false|\"deletion_protection\.enabled\"[^}]*\"false\""}, "turns off deletion protection of a load balancer")]),
}

PUBLIC_MEMBERS = ["allUsers", "allAuthenticatedUsers"]
GCP_COND = {
    "setIamPolicy": (None, [(X, f("policy.bindings.*.members.*", PUBLIC_MEMBERS), "gives everyone a role on {path}")]),
    "compute.instances.setDeletionProtection": (None, [(O, {"query": {"deletionProtection": FALSE}}, "turns off deletion protection of the instance {path}")]),
    "compute.instances.setMetadata": (None, [(X, f("items.*.key", ["ssh-keys", "sshKeys"]), "sets SSH keys on the instance {path}"),
                                             (X, [f("items.*.key", ["enable-oslogin"]), f("items.*.value", FALSE + ["FALSE"])], "turns off OS Login on the instance {path}")]),
    "compute.projects.setCommonInstanceMetadata": (None, [(X, f("items.*.key", ["ssh-keys", "sshKeys"]), "sets SSH keys for every instance of the project {path}"),
                                                          (X, [f("items.*.key", ["enable-oslogin"]), f("items.*.value", FALSE + ["FALSE"])], "turns off OS Login for the project {path}")]),
    "compute.instanceGroupManagers.resize": (None, [(O, {"query": {"size": ZERO}}, "resizes the instance group {path} to zero")]),
    "compute.regionInstanceGroupManagers.resize": (None, [(O, {"query": {"size": ZERO}}, "resizes the instance group {path} to zero")]),
    "compute.firewalls.insert": (None, [(X, f("sourceRanges.*", ["0.0.0.0/0", "::/0"]), "opens a firewall rule to the whole Internet")]),
    "compute.firewalls.patch": (None, [(X, f("sourceRanges.*", ["0.0.0.0/0", "::/0"]), "opens the firewall rule {path} to the whole Internet")]),
    "compute.firewalls.update": (None, [(X, f("sourceRanges.*", ["0.0.0.0/0", "::/0"]), "opens the firewall rule {path} to the whole Internet")]),
    "storage.buckets.patch": (None, [(X, f("iamConfiguration.publicAccessPrevention", ["inherited", "unspecified"]), "turns off public access prevention of the bucket {path}"),
                                     (O, f("versioning.enabled", FALSE), "turns off versioning of the bucket {path}"),
                                     (G, f("softDeletePolicy.retentionDurationSeconds", ZERO), "turns off soft delete of the bucket {path}")]),
    "storage.buckets.update": (None, [(X, f("iamConfiguration.publicAccessPrevention", ["inherited", "unspecified"]), "turns off public access prevention of the bucket {path}"),
                                      (O, f("versioning.enabled", FALSE), "turns off versioning of the bucket {path}"),
                                      (G, f("softDeletePolicy.retentionDurationSeconds", ZERO), "turns off soft delete of the bucket {path}")]),
    "sql.instances.patch": (None, [(X, f("settings.ipConfiguration.authorizedNetworks.*.value", ["0.0.0.0/0"]), "opens the Cloud SQL instance {path} to the whole Internet"),
                                   (O, f("settings.deletionProtectionEnabled", FALSE), "turns off deletion protection of the Cloud SQL instance {path}"),
                                   (G, f("settings.backupConfiguration.enabled", FALSE), "turns off backups of the Cloud SQL instance {path}"),
                                   (O, f("settings.activationPolicy", ["NEVER"]), "stops the Cloud SQL instance {path}")]),
    "sql.instances.update": (None, [(X, f("settings.ipConfiguration.authorizedNetworks.*.value", ["0.0.0.0/0"]), "opens the Cloud SQL instance {path} to the whole Internet"),
                                    (O, f("settings.deletionProtectionEnabled", FALSE), "turns off deletion protection of the Cloud SQL instance {path}"),
                                    (G, f("settings.backupConfiguration.enabled", FALSE), "turns off backups of the Cloud SQL instance {path}"),
                                    (O, f("settings.activationPolicy", ["NEVER"]), "stops the Cloud SQL instance {path}")]),
    "sql.instances.executeSql": (None, [(G, "sql:field:sqlStatement:postgres", "runs {sql_kind} on {objects} in the Cloud SQL instance {instance}")]),
    "container.projects.locations.clusters.update": (None, [(X, f("update.desiredLegacyAbac.enabled", TRUE), "turns on legacy ABAC on the GKE cluster {path}"),
                                                            (X, f("update.desiredMasterAuthorizedNetworksConfig.cidrBlocks.*.cidrBlock", ["0.0.0.0/0"]), "opens the GKE control plane {path} to the whole Internet")]),
    "container.projects.zones.clusters.update": (None, [(X, f("update.desiredLegacyAbac.enabled", TRUE), "turns on legacy ABAC on the GKE cluster {path}"),
                                                        (X, f("update.desiredMasterAuthorizedNetworksConfig.cidrBlocks.*.cidrBlock", ["0.0.0.0/0"]), "opens the GKE control plane {path} to the whole Internet")]),
    "container.projects.locations.clusters.setLegacyAbac": (None, [(X, f("enabled", TRUE), "turns on legacy ABAC on the GKE cluster {path}")]),
    "container.projects.zones.clusters.legacyAbac": (None, [(X, f("enabled", TRUE), "turns on legacy ABAC on the GKE cluster {path}")]),
    "container.projects.locations.clusters.nodePools.setSize": (None, [(O, f("nodeCount", ZERO), "sets the GKE node pool {path} to zero nodes")]),
    "container.projects.zones.clusters.nodePools.setSize": (None, [(O, f("nodeCount", ZERO), "sets the GKE node pool {path} to zero nodes")]),
    "bigquery.jobs.insert": (None, [(G, "sql:field:configuration.query.query:bigquery", "runs {sql_kind} on {objects} in BigQuery")]),
    "bigquery.jobs.query": (None, [(G, "sql:field:query:bigquery", "runs {sql_kind} on {objects} in BigQuery")]),
    "bigquery.datasets.patch": (None, [(X, f("access.*.specialGroup", ["allAuthenticatedUsers"]), "gives every Google account access to the dataset {path}"),
                                       (X, f("access.*.iamMember", PUBLIC_MEMBERS), "gives everyone access to the dataset {path}")]),
    "bigquery.datasets.update": (None, [(X, f("access.*.specialGroup", ["allAuthenticatedUsers"]), "gives every Google account access to the dataset {path}"),
                                        (X, f("access.*.iamMember", PUBLIC_MEMBERS), "gives everyone access to the dataset {path}")]),
    "dns.changes.create": (None, [(O, present("deletions"), "deletes DNS records in the zone {path}")]),
    "cloudkms.projects.locations.keyRings.cryptoKeys.cryptoKeyVersions.patch": (None, [(O, f("state", ["DISABLED"]), "disables the key version {path}; what it encrypted cannot be read")]),
    "firestore.projects.databases.documents.batchWrite": (None, [(G, present("writes.*.delete"), "deletes Firestore documents")]),
    "firestore.projects.databases.documents.commit": (None, [(G, present("writes.*.delete"), "deletes Firestore documents")]),
    "firestore.projects.databases.patch": (None, [(O, f("deleteProtectionState", ["DELETE_PROTECTION_DISABLED"]), "turns off delete protection of the Firestore database {path}")]),
    "storage.projects.hmacKeys.update": (None, [(O, f("state", ["INACTIVE"]), "deactivates an HMAC key; whatever uses it stops"),
                                                (X, f("state", ["ACTIVE"]), "reactivates an HMAC key")]),
}
for _k in ("bucketAccessControls", "defaultObjectAccessControls", "objectAccessControls"):
    for _v in ("insert", "patch", "update"):
        GCP_COND["storage.%s.%s" % (_k, _v)] = (None, [(X, f("entity", PUBLIC_MEMBERS), "grants {path} to everyone")])

AZ_COND = {  # by resource type
    "firewallrules": (None, [(X, [f("properties.startIpAddress", ["0.0.0.0"]), f("properties.endIpAddress", ["255.255.255.255"])], "opens a database firewall to every address")]),
    "securityrules": (None, [(X, [f("properties.access", ["Allow"]), f("properties.direction", ["Inbound"]), f("properties.sourceAddressPrefix", ANYONE)], "lets traffic in from the whole Internet"),
                             (X, [f("properties.access", ["Allow"]), f("properties.direction", ["Inbound"]), f("properties.sourceAddressPrefixes.*", ANYONE)], "lets traffic in from the whole Internet")]),
    "storageaccounts": (None, [(X, f("properties.allowBlobPublicAccess", TRUE), "allows public blob access on the storage account {path}"),
                               (X, f("properties.networkAcls.defaultAction", ["Allow"]), "opens the storage account {path} to every network")]),
    "containers": (None, [(X, f("properties.publicAccess", ["Container", "Blob"]), "makes the blob container {path} public")]),
    "vaults": (None, [(X, present("properties.accessPolicies"), "gives someone access to the key vault {path}"),
                      (O, f("properties.enableSoftDelete", FALSE), "turns off soft delete of the key vault {path}")]),
    "registries": (None, [(X, f("properties.adminUserEnabled", TRUE), "turns on the admin user of the registry {path}"),
                          (X, f("properties.anonymousPullEnabled", TRUE), "lets anyone pull from the registry {path}")]),
    "managedclusters": (None, [(X, f("properties.apiServerAccessProfile.authorizedIPRanges.*", ["0.0.0.0/0"]), "opens the AKS API server {path} to the whole Internet")]),
    "agentpools": (None, [(O, f("properties.count", ZERO), "scales the AKS node pool {path} to zero")]),
    "servers": (None, [(X, f("properties.publicNetworkAccess", ["Enabled"]), "turns on public network access of the server {path}")]),
    "flexibleservers": (None, [(X, f("properties.network.publicNetworkAccess", ["Enabled"]), "turns on public network access of the server {path}")]),
    "databaseaccounts": (None, [(X, f("properties.publicNetworkAccess", ["Enabled"]), "turns on public network access of the Cosmos DB account {path}")]),
    "redis": (None, [(X, f("properties.enableNonSslPort", TRUE), "opens the non-TLS port of the cache {path}")]),
    "disks": (None, [(X, f("properties.networkAccessPolicy", ["AllowAll"]), "lets anyone with a link export the disk {path}")]),
    "virtualmachinescalesets": (None, [(O, f("sku.capacity", ZERO), "scales the scale set {path} to zero")]),
    "deployments": (None, [(G, f("properties.mode", ["Complete"]), "deploys a template in Complete mode, deleting what it does not list in {path}")]),
    "authsettings": (None, [(X, f("properties.enabled", FALSE), "turns off authentication of the app {path}")]),
    "authsettingsv2": (None, [(X, f("properties.platform.enabled", FALSE), "turns off authentication of the app {path}")]),
    "blobservices": (None, [(O, f("properties.deleteRetentionPolicy.enabled", FALSE), "turns off soft delete of blobs in {path}"),
                            (O, f("properties.containerDeleteRetentionPolicy.enabled", FALSE), "turns off soft delete of containers in {path}")]),
    "managementpolicies": (None, [(G, rx(r'"delete"\s*:'), "sets a lifecycle policy that deletes blobs in {path}")]),
    "deploymentstacks": (None, [(G, {"query": {"unmanageAction.Resources": ["delete"]}}, "updates the deployment stack {path}, deleting resources it no longer lists"),
                                (G, f("properties.actionOnUnmanage.resources", ["delete"]), "updates the deployment stack {path}, deleting resources it no longer lists")]),
}

K8S_COND = {  # by the resource the path ends in
    "scale": (None, [(O, f("spec.replicas", ZERO), "scales {path} to zero")]),
    "deployments": (None, [(O, f("spec.replicas", ZERO), "scales the deployment {path} to zero")]),
    "statefulsets": (None, [(O, f("spec.replicas", ZERO), "scales the stateful set {path} to zero")]),
    "replicasets": (None, [(O, f("spec.replicas", ZERO), "scales the replica set {path} to zero")]),
    "replicationcontrollers": (None, [(O, f("spec.replicas", ZERO), "scales {path} to zero")]),
    "services": (None, [(X, f("spec.type", ["LoadBalancer", "NodePort"]), "exposes the service {path} outside the cluster")]),
    "nodes": (None, [(O, f("spec.unschedulable", TRUE), "cordons the node {path}"),
                     (O, f("spec.taints.*.effect", ["NoExecute"]), "taints the node {path} NoExecute, which evicts its pods")]),
}

GH_COND = {
    "PATCH /repos/{owner}/{repo}": (None, [(X, f("visibility", ["public"]), "makes the repository {owner}/{repo} public"),
                                           (X, f("private", FALSE), "makes the repository {owner}/{repo} public"),
                                           (O, f("archived", TRUE), "archives {owner}/{repo}, which makes it read-only")]),
    "PATCH /repos/{owner}/{repo}/git/refs/{ref}": (None, [(R, f("force", TRUE), "force-moves {ref} of {owner}/{repo}")]),
    "PUT /orgs/{org}/actions/permissions/workflow": (None, [(X, f("default_workflow_permissions", ["write"]), "gives GitHub Actions in {org} write access by default"),
                                                            (X, f("can_approve_pull_request_reviews", TRUE), "lets GitHub Actions in {org} approve pull requests")]),
    "PUT /repos/{owner}/{repo}/actions/permissions/workflow": (None, [(X, f("default_workflow_permissions", ["write"]), "gives GitHub Actions in {owner}/{repo} write access by default"),
                                                                      (X, f("can_approve_pull_request_reviews", TRUE), "lets GitHub Actions in {owner}/{repo} approve pull requests")]),
}
GH_GQL_COND = {
    "updateRefs": (None, [(R, {"input.refUpdates.*.afterOid": ["0" * 40]}, "deletes refs through updateRefs"),
                          (R, {"input.refUpdates.*.force": TRUE}, "force-moves refs through updateRefs")]),
    "updateRef": (None, [(R, {"input.force": TRUE}, "force-moves a ref through updateRef")]),
}

# Data planes the management surfaces do not list, written by hand from the REST references.
AZ_DATA = [
    {"id": "azure-blob", "name": "Azure Blob Storage", "hosts": ["*.blob.core.windows.net"], "unknown": "hold", "rules": [
        {"id": "azblob.container-delete", "match": {"rest": {"method": "DELETE", "path": "/{container}", "query": {"restype": ["container"]}}}, "outcome": G,
         "say": "deletes the blob container {container} and every blob in it"},
        {"id": "azblob.blob-delete", "match": {"rest": {"method": "DELETE", "path": "/{container}/{blob+}"}}, "outcome": G,
         "say": "deletes the blob {container}/{blob}"},
        {"id": "azblob.batch", "match": {"rest": {"method": "POST", "path": ["/", "/{container}"], "query": {"comp": ["batch"]}, "body": rx(r"DELETE /")}}, "outcome": G,
         "say": "deletes blobs in a batch"},
        {"id": "azblob.container-acl", "match": {"rest": {"method": "PUT", "path": "/{container}", "query": {"comp": ["acl"]}}}, "outcome": X,
         "say": "changes who can read the blob container {container}"},
    ]},
    {"id": "azure-keyvault", "name": "Azure Key Vault", "hosts": ["*.vault.azure.net"], "unknown": "hold", "rules": [
        {"id": "akv.purge", "match": {"rest": {"method": "DELETE", "path": ["/deletedsecrets/{name}", "/deletedkeys/{name}", "/deletedcertificates/{name}"]}}, "outcome": G,
         "say": "purges the deleted Key Vault item {name}; it cannot be recovered"},
        {"id": "akv.delete", "match": {"rest": {"method": "DELETE", "path": ["/secrets/{name}", "/keys/{name}", "/certificates/{name}"]}}, "outcome": R,
         "say": "deletes the Key Vault item {name} (recoverable while soft delete keeps it)"},
    ]},
]


# ---------------------------------------------------------------- helpers
def tpl(prov, path):
    segs = path.strip("/").split("/")
    out = []
    for i, s in enumerate(segs):
        m = re.fullmatch(r"\{([^}]+)\}", s)
        if m:
            name = re.sub(r"[^A-Za-z0-9_]", "_", m.group(1))
            last = i == len(segs) - 1
            multi = (prov == "azure" and i == 0) or (last and name in ("ref", "path")) or (prov == "github" and name == "ref")
            out.append("{%s%s}" % (name, "+" if multi else ""))
        elif "{" in s:   # Google's {x}:verb
            out.append(re.sub(r"\{[^}]+\}", "*", s))
        else:
            out.append(s)
    return "/" + "/".join(out)


def outcome_of(label):
    return label.split(":", 1)[1] if label.startswith("hold:") else None


def rest_rule(rid, method, path, outcome, say, body=None, query=None, action="hold"):
    m = {"method": method, "path": path}
    if re.search(r"/\{[^}/]+\}$", path) and "googleapis" in CUR.get("host", ""):
        m["no_verb"] = True
    if body is not None:
        m["body"] = body
    if query:
        m["query"] = query
    r = {"id": rid, "match": {"rest": m}, "outcome": outcome, "say": say}
    if action != "hold":
        r["action"] = action
    return r


def cond_rules(prov, rid, method, path, conds, default, sayname):
    out = []
    for i, (outcome, c, say) in enumerate(conds[1]):
        cid = "%s.c%d" % (rid, i + 1)
        if isinstance(c, str) and c.startswith("sql:"):
            _, _, field, dialect = c.split(":")
            out.append({"id": cid, "match": {"sql": {"method": [method], "path": path, "shape": "field:" + field, "dialect": dialect}},
                        "outcome": outcome, "say": say})
        elif isinstance(c, dict) and "query" in c:
            out.append(rest_rule(cid, method, path, outcome, say, query=c["query"]))
        else:
            out.append(rest_rule(cid, method, path, outcome, say, body=c))
    d = conds[0]
    if d:
        out.append(rest_rule(rid, method, path, d, "%s on {path} (%s)" % (sayname, d)))
    else:
        out.append(rest_rule(rid, method, path, None, "%s (a documented operation)" % sayname, action="allow"))
    return out


CUR = {}


def surface_rules(prov):
    ops = json.load(open(os.path.join(SURF, "ops", prov + ".json")))
    labels = S.read_labels(prov)
    rules, seen = [], set()
    for op in ops:
        CUR["host"] = op.get("host", "")
        key = S.key_of(prov, op)
        if key in seen:
            continue
        seen.add(key)
        label = labels[key][0]
        rid = "%s:%s" % (prov, key)
        path = tpl(prov, op["path"])
        sayname = "calls %s %s" % (NAMES[prov], op["name"] if prov in ("gcloud", "kubernetes") else key)
        if label == "cond":
            if prov == "gcloud":
                c = GCP_COND.get(key) or GCP_COND[key.split(".")[-1]]
            elif prov == "azure":
                segs = op["path"].strip("/").split("/")
                c = AZ_COND[segs[-2].lower() if segs[-1].startswith("{") else segs[-1].lower()]
            elif prov == "kubernetes":
                c = K8S_COND[[s for s in op["path"].strip("/").split("/") if not s.startswith("{")][-1]]
            else:
                c = GH_COND[key]
            rules += cond_rules(prov, rid, op["method"], path, c, None, sayname)
            continue
        out = outcome_of(label)
        if out:
            rules.append(rest_rule(rid, op["method"], path, out, "%s on {path} (%s)" % (sayname, out)))
        else:
            rules.append(rest_rule(rid, op["method"], path, None, "%s (%s)" % (sayname, label), action="allow"))
    return rules


def gcp_apis():
    ops = json.load(open(os.path.join(SURF, "ops", "gcloud.json")))
    rules = surface_rules("gcloud")
    host_of = {}
    for op in ops:
        host_of["gcloud:" + op["name"]] = op["host"]
    apis = {}
    for r in rules:
        key = r["id"].rsplit(".c", 1)[0] if re.search(r"\.c\d+$", r["id"]) else r["id"]
        h = host_of[key]
        a = apis.setdefault(h, {"id": "gcp-" + h.split(".")[0], "name": "Google Cloud " + h.split(".")[0],
                                "hosts": [h, "*-" + h], "unknown": "hold", "rules": []})
        a["rules"].append(r)
    return list(apis.values())


def gql_rules():
    labels = S.read_labels("github-gql")
    out, allow = [], []
    for name, (label, why) in sorted(labels.items()):
        if label == "cond":
            for i, (outcome, arg, say) in enumerate(GH_GQL_COND[name][1]):
                out.append({"id": "github-gql:%s.c%d" % (name, i + 1), "match": {"graphql": {"type": "mutation", "field": name, "arg": arg}},
                            "outcome": outcome, "say": say})
            allow.append(name)
        elif outcome_of(label):
            out.append({"id": "github-gql:" + name, "match": {"graphql": {"type": "mutation", "field": name}},
                        "outcome": outcome_of(label), "say": "calls the GitHub GraphQL mutation %s (%s)" % (name, outcome_of(label))})
        else:
            allow.append(name)
    out.append({"id": "github-gql:documented", "match": {"graphql": {"type": "mutation", "field_in": sorted(allow)}},
                "outcome": None, "action": "allow", "say": "a documented GitHub mutation Silo lets through"})
    return out


# ---------------------------------------------------------------- AWS
def aws_labelled():
    sys.path.insert(0, "/usr/local/lib/python3.13/site-packages/awscli")
    sys.path.insert(0, os.path.join(HERE, "..", "..", "..", "2026-11-24-destructive-calls", "proto"))
    import botocore.session
    import aws_destructive as AD
    sess = botocore.session.get_session()
    labels = S.read_labels("aws")
    services = sorted({k.split(" ")[0] for k in labels})
    ops, params = [], []
    for svc in services:
        model = sess.get_service_model(svc)._service_description
        md = model["metadata"]
        for name in sorted(model["operations"]):
            key = svc + " " + name
            label = labels.get(key, ("allow", "a read"))[0]
            match = AD.match_for(model, name)
            for p, m in match.items():
                if m.get("required_query_key", "") and m["required_query_key"].startswith("x-id="):
                    m["required_query_key"] = None
            o = {"service": svc, "endpoint_prefix": md.get("endpointPrefix"), "signing_name": md.get("signingName", md.get("endpointPrefix")),
                 "operation": name, "match": match}
            if label == "cond":
                d, conds = AWS_COND[key]
                o["outcome"] = d
                if not d:
                    o["action"] = "allow"
                # lower case: #353's _rest_names compares it with the lowered query keys
                rest = next(({"method": m["method"], "path": m["path_template"].split("?")[0],
                              "query_key": (m.get("required_query_key") or "").split("=")[0].lower() or None}
                             for p, m in match.items() if p in ("rest-json", "rest-xml")), None)
                for outcome, c, say in conds:
                    # #353 names the service of a parameter rule as the request does (signing name)
                    p = dict(c, service=md.get("signingName", md.get("endpointPrefix")), operation=name, outcome=outcome, say=say)
                    if rest:
                        p["rest"] = rest
                    params.append(p)
            elif outcome_of(label):
                o["outcome"] = outcome_of(label)
            else:
                o["action"] = "allow"
            ops.append(o)
    missing = [k for k in labels if labels[k][0] == "cond" and k not in AWS_COND]
    assert not missing, missing
    with open(os.path.join(HERE, "aws_labelled.json"), "w") as fh:
        json.dump({"about": "Every operation of the %d AWS services labelled in ../surface/labels/aws.txt, with its wire form from "
                            "botocore (awscli 1.46.3's copy) and the label's outcome, or allow. Made by rules_build.py." % len(services),
                   "services": services, "operations": ops}, fh, indent=0)
    return services, params


# ---------------------------------------------------------------- tuned after the captures (empty until then)
TUNED = {}


def build(tuned):
    src = json.load(open(os.path.join(P383, "rules.json")))
    out = copy.deepcopy(src)
    out["about"] = (src.get("about", "") + " Extended on 2026-11-25 for AWS, Google Cloud, Azure, GitHub and Kubernetes "
                    "from their documented surfaces (research/2026-11-25-destructive-calls-clouds).")
    for api in out["apis"]:
        g = api.get("graphql")
        if g and g.get("known_mutations") and not g["known_mutations"].startswith("../"):
            g["known_mutations"] = P383P + g["known_mutations"]
        for r in api.get("rules", []):
            ff = r["match"].get("graphql", {}).get("field_from")
            if ff and not ff["file"].startswith("../"):
                ff["file"] = P383P + ff["file"]
        if api["id"] == "aws":
            services, params = aws_labelled()
            api["aws"]["labelled"] = "aws_labelled.json"
            api["aws"]["params"] = api["aws"].get("params", []) + params
        if api["id"] == "github":
            new = surface_rules("github") + gql_rules()
            have = {r["id"] for r in api["rules"]}
            assert not have & {r["id"] for r in new}
            api["rules"] = [r for r in api["rules"] if not r.get("fallback")] + new + [r for r in api["rules"] if r.get("fallback")]
    out["apis"] += gcp_apis()
    out["apis"].append({"id": "azure", "name": "Azure Resource Manager", "hosts": ["management.azure.com"], "unknown": "hold",
                        "rules": surface_rules("azure")})
    out["apis"] += copy.deepcopy(AZ_DATA)
    out["apis"].append({"id": "kubernetes", "name": "Kubernetes API", "unknown": "hold",
                        "hosts": ["*.eks.amazonaws.com", "*.azmk8s.io", "*.gke.goog", "kubernetes.default.svc", "kubernetes.default.svc.cluster.local", "kube.capture.test"],
                        "rules": surface_rules("kubernetes")})
    # the any-host MCP sniffer stays last
    out["apis"] = [a for a in out["apis"] if not a.get("mcp_sniff")] + [a for a in out["apis"] if a.get("mcp_sniff")]
    if tuned:
        for api in out["apis"]:
            api["rules"] = TUNED.get(api["id"], []) + api.get("rules", [])
    name = "rules.json" if tuned else "rules-blind.json"
    with open(os.path.join(HERE, name), "w") as fh:
        json.dump(out, fh, indent=0)
    print(name + ":", sum(len(a.get("rules", [])) for a in out["apis"]), "rules,", len(out["apis"]), "APIs")


if __name__ == "__main__":
    build("--tuned" in sys.argv)
