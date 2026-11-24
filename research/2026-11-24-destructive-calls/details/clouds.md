# Destructive calls on AWS, Google Cloud and Azure

All sources were read on 2026-11-24 unless a line says otherwise. "botocore" means the botocore
1.44.10 wheel from PyPI, whose `botocore/data/*/service-2.json.gz` files are the AWS service models
the AWS CLI and boto3 use. "Discovery" means Google's API discovery documents at
`https://<service>.googleapis.com/$discovery/rest?version=<v>`.

The outcome words (gone, recoverable, outage, money) are the ones in the shared brief.

Files that go with this page:

- `proto/aws_destructive.py` reads the botocore models and writes `proto/aws_destructive.json`.
  It lists every operation whose name starts with Delete, Terminate, Remove, Purge, Drop, Destroy,
  Disable, Revoke, Detach, Deregister, Cancel, Reset or Restore, plus a short hand list of others
  (ScheduleKeyDeletion, ReleaseAddress, CloseAccount, BatchWriteItem and so on). For each one it
  writes how to match it on the wire for every protocol the service speaks. It also writes 50
  hand-checked "top" operations with an outcome and 32 destructive *parameters*. It asserts that
  every hand-named operation and parameter exists in the models, so a rename upstream breaks the
  script instead of silently matching nothing.

## 1. AWS: how the operation shows on the wire

### What the models say

botocore holds 445 services and 20,077 operations. The name filter finds 3,008 destructive
candidates: 2,435 Delete, 147 Cancel, 87 Remove, 85 Disable, 56 Deregister, 47 Restore, 38 Reset,
35 Detach, 30 Revoke, 24 Terminate, 1 Purge and 23 others. The filter over-counts. DeleteTags and
CancelExportTask are on the list, and the 47 Restore operations mostly *create* a resource from a
backup. The `top` and `destructive_params` lists are the reviewed part.

Services by protocol (from `metadata.protocols`):

| Protocols listed | Services |
|---|---|
| rest-json | 265 |
| json | 113 |
| smithy-rpc-v2-cbor + json | 40 |
| query | 17 |
| rest-xml | 4 (S3, S3 Control, CloudFront, Route 53) |
| smithy-rpc-v2-cbor only | 4 |
| smithy-rpc-v2-cbor + json + query | 1 (CloudWatch) |
| ec2 | 1 |

Destructive candidates by protocol: rest-json 1,473, json 1,048, smithy-rpc-v2-cbor 247, query 232,
ec2 167, rest-xml 80. A service that lists several protocols counts once in each.

### How each protocol names the operation

These rules come from `botocore/serialize.py` (botocore 1.44.10). The host gives the service and
the region, for example `rds.eu-west-3.amazonaws.com`. The signature's credential scope
(`Credential=<key id>/<date>/<region>/<service>/aws4_request`) repeats the region and the
signing name.

| Protocol | Operation is in | Example |
|---|---|---|
| query, ec2 | Form body `Action=<Op>&Version=<apiVersion>`, POST `/`. AWS also accepts the same pairs in a GET query string ([DeleteDBInstance sample request](https://docs.aws.amazon.com/AmazonRDS/latest/APIReference/API_DeleteDBInstance.html)). | `Action=DeleteDBInstance&Version=2014-10-31&DBInstanceIdentifier=prod&SkipFinalSnapshot=true` |
| json | Header `X-Amz-Target: <targetPrefix>.<Op>`, POST `/`, `Content-Type: application/x-amz-json-1.0` or `1.1` | `X-Amz-Target: DynamoDB_20120810.DeleteTable`; `X-Amz-Target: secretsmanager.DeleteSecret` ([sample](https://docs.aws.amazon.com/secretsmanager/latest/apireference/API_DeleteSecret.html)) |
| rest-json, rest-xml | Method and path template, sometimes a required query key | Lambda `DELETE /2015-03-31/functions/{FunctionName}`; EKS `DELETE /clusters/{name}`; S3 `DELETE /{Bucket}`, `POST /{Bucket}?delete` |
| smithy-rpc-v2-cbor | Path `/service/<targetPrefix>/operation/<Op>`, POST, header `smithy-protocol: rpc-v2-cbor`, CBOR body | `POST /service/GraniteServiceVersion20100801/operation/DeleteAlarms` (CloudWatch) |

Three traps for a matcher:

- **One service, several protocols.** CloudWatch lists CBOR, JSON and query. botocore picks the
  first protocol it supports in the order json, rest-json, rest-xml, smithy-rpc-v2-cbor, query, ec2
  (`PRIORITY_ORDERED_SUPPORTED_PROTOCOLS` in `botocore/utils.py`). So boto3 sends CloudWatch as JSON
  with `X-Amz-Target: GraniteServiceVersion20100801.DeleteAlarms`, an older SDK sends
  `Action=DeleteAlarms`, and another SDK may send CBOR. The JSON file lists every form.
- **The CBOR path uses `targetPrefix`, not the service name.** CloudWatch's is
  `GraniteServiceVersion20100801`, Transcribe's is `Transcribe`, EventBridgeV2's is `AWSEventsV2`.
  The 41 services with CBOR in this botocore include CodeBuild, CodeDeploy, CodePipeline, Directory
  Service, GameLift, Keyspaces, MemoryDB, Network Firewall, Service Catalog, Cloud Map, Shield,
  Textract, Transcribe, Translate and WorkSpaces. The full list is in the JSON.
- **S3 has two addressings.** Path style puts the bucket first in the path (`/{Bucket}/{Key+}`).
  Virtual-hosted style puts it in the host (`<bucket>.s3.<region>.amazonaws.com`) and the path
  starts at the key. The JSON gives both forms for every S3 operation. A sub-resource such as
  `?delete`, `?versioning` or `?lifecycle` is a query key with no value.

Two more paths reach many services at once:

- **Cloud Control API.** `X-Amz-Target: CloudApiService.DeleteResource` with a `TypeName` such as
  `AWS::RDS::DBInstance` deletes a resource of any type. A matcher keyed only on per-service actions
  misses it. Pulumi's AWS Native provider uses this API (from Pulumi's description of the provider,
  not checked on the wire: unverified).
- **CloudFormation.** `Action=DeleteStack` deletes every resource in a stack, and
  `Action=ExecuteChangeSet` or `Action=UpdateStack` can remove or replace any of them. One call on
  the wire stands for many deletions on the AWS side.

### Destructive operations that matter most for agents

The outcome assumes the default settings. "Undo by Silo" says whether Silo could make the call
undoable with a call of its own before it lets it pass, or by rewriting it (see section 2 for why
Silo can rewrite AWS requests only if Silo signs them).

| Operation | On the wire | Outcome | Provider-side undo | Undo by Silo |
|---|---|---|---|---|
| S3 DeleteObjects | `POST /{Bucket}?delete`, XML list of up to 1000 keys | gone, or recoverable if versioning is on | With versioning on, a delete without a version id only adds a delete marker; a delete *with* a version id is permanent ([S3 guide](https://docs.aws.amazon.com/AmazonS3/latest/userguide/DeletingObjectVersions.html)). Object Lock and MFA delete block it. | Yes, cheap: `PUT /{Bucket}?versioning` with `Status=Enabled` first, then let the delete pass. Deletes then become delete markers. Versioning can afterwards only be suspended, never removed, and old versions cost storage. Not for deletes that name a VersionId. |
| S3 DeleteObject | `DELETE /{Bucket}/{Key+}` | same | same | same |
| S3 DeleteBucket | `DELETE /{Bucket}` | outage (bucket must be empty; the loss happened in the deletes before) | none | n/a |
| S3 PutBucketVersioning | `PUT /{Bucket}?versioning`, `<Status>Suspended</Status>` | gone, later | n/a | Hold it. |
| S3 PutBucketLifecycleConfiguration | `PUT /{Bucket}?lifecycle`, `<Expiration>` or `<NoncurrentVersionExpiration>` | gone, later and asynchronous | `NoncurrentVersionExpiration` removes versions for good ([S3 guide](https://docs.aws.amazon.com/AmazonS3/latest/userguide/DeletingObjectVersions.html)) | Hold it. |
| RDS DeleteDBInstance | `Action=DeleteDBInstance`, params `SkipFinalSnapshot`, `FinalDBSnapshotIdentifier`, `DeleteAutomatedBackups` | gone with `SkipFinalSnapshot=true` and the default `DeleteAutomatedBackups` | "When you delete a DB instance, all automated backups for that instance are deleted and can't be recovered", unless retained; manual snapshots stay; the default is to remove automated backups ([API](https://docs.aws.amazon.com/AmazonRDS/latest/APIReference/API_DeleteDBInstance.html)). Refused while deletion protection is on. Backups copied to another region survive. AWS Support "might" recover it "for up to six days after the deletion request" ([guide](https://docs.aws.amazon.com/AmazonRDS/latest/UserGuide/USER_DeleteInstance.html)). | Yes, best by rewrite: set `SkipFinalSnapshot=false`, add `FinalDBSnapshotIdentifier=silo-...`, set `DeleteAutomatedBackups=false`. RDS then takes the snapshot itself, with no wait for Silo. |
| RDS DeleteDBCluster | `Action=DeleteDBCluster`, same three params | same | same | same |
| RDS ModifyDBInstance / ModifyDBCluster | `DeletionProtection=false`, `BackupRetentionPeriod=0` | prepares gone | n/a | Hold it. |
| RDS DeleteDBSnapshot / DeleteDBClusterSnapshot | `Action=...` | gone | none (manual snapshots have no recycle bin) | Copy first (CopyDBSnapshot), slow. |
| DynamoDB DeleteTable | `X-Amz-Target: DynamoDB_20120810.DeleteTable` | gone without PITR; recoverable with PITR | With PITR on, DynamoDB keeps a system backup `<table>$DeletedTableBackup` for 35 days at no cost ([guide](https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/PointInTimeRecovery_Howitworks.html)). Refused while `DeletionProtectionEnabled`. | Yes: `CreateBackup` first. Backups are "processed instantaneously and become available for restore within minutes", and the source table cannot be deleted while a backup is in progress ([guide](https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/CreateBackup.html)). Silo must wait for the backup before it releases the delete. |
| DynamoDB UpdateTable / UpdateContinuousBackups | `DeletionProtectionEnabled=false`; `PointInTimeRecoveryEnabled=false` | prepares gone | n/a | Hold it. |
| DynamoDB BatchWriteItem | `X-Amz-Target: DynamoDB_20120810.BatchWriteItem` with `DeleteRequest` entries | gone (items) | PITR restores to a new table | Only a backup first. |
| EC2 TerminateInstances | `Action=TerminateInstances&InstanceId.1=...` | gone for volumes with DeleteOnTermination (root volume by default) | Refused while `DisableApiTermination` is set | Yes: `CreateSnapshots` (multi-volume) first. Snapshots are point-in-time at the request and stay `pending` "several hours" while data moves ([EBS guide](https://docs.aws.amazon.com/ebs/latest/userguide/ebs-creating-snapshot.html)). Whether the volume may be deleted while its snapshot is pending: unverified. |
| EC2 DeleteVolume | `Action=DeleteVolume` | gone, or recoverable with a Recycle Bin rule | EC2 Recycle Bin keeps volumes, snapshots and EBS-backed AMIs that match a retention rule ([guide](https://docs.aws.amazon.com/AWSEC2/latest/UserGuide/recycle-bin.html)) | CreateSnapshot first, or one Recycle Bin rule set up once |
| EC2 DeleteSnapshot | `Action=DeleteSnapshot` | recoverable only with a Recycle Bin rule | same | same |
| Lambda DeleteFunction | `DELETE /2015-03-31/functions/{FunctionName}` | outage (code is in the repo) | none | `GET /2015-03-31/functions/{name}` returns a code download URL; Silo could save it. |
| Lambda PutFunctionConcurrency | `PUT /2017-10-31/functions/{FunctionName}/concurrency`, `ReservedConcurrentExecutions=0` | outage | n/a | n/a |
| ECS DeleteService / UpdateService / DeleteCluster | `X-Amz-Target: AmazonEC2ContainerServiceV20141113.DeleteService` (`force`), `desiredCount=0` | outage | none | n/a |
| EKS DeleteCluster | `DELETE /clusters/{name}` | outage, and all Kubernetes objects in etcd are gone | Deletion protection since August 2025, off by default; turning it off needs `UpdateClusterConfig` ([guide](https://docs.aws.amazon.com/eks/latest/userguide/deletion-protection.html)) | No AWS-side snapshot of etcd. |
| CloudFormation DeleteStack | `Action=DeleteStack`, `DeletionMode=FORCE_DELETE_STACK`, `RetainResources` | gone for every resource without `DeletionPolicy: Retain/Snapshot` | Default policy is Delete, except Snapshot for `AWS::RDS::DBCluster` and standalone `AWS::RDS::DBInstance`. CloudFormation deletes Secrets Manager secrets with `ForceDeleteWithoutRecovery` by default ([DeletionPolicy](https://docs.aws.amazon.com/AWSCloudFormation/latest/UserGuide/aws-attribute-deletionpolicy.html)). Termination protection refuses it. | Silo can read the stack's resources (`DescribeStackResources`) to show the ask. |
| CloudFormation ExecuteChangeSet / UpdateStack | `Action=ExecuteChangeSet` | gone for removed or replaced resources | same | Silo can call `DescribeChangeSet` itself and show each `ResourceChange` with `Action=Remove`, `Replacement=True` and `PolicyAction` (Delete, Retain, Snapshot, ReplaceAndDelete, ...) (botocore model). |
| Route 53 ChangeResourceRecordSets | `POST /2013-04-01/hostedzone/{Id}/rrset/`, XML `<Action>DELETE</Action>` (or UPSERT) | outage | none | Silo can save the record set first (`ListResourceRecordSets`). |
| Route 53 DeleteHostedZone | `DELETE /2013-04-01/hostedzone/{Id}` | outage; a new zone gets new name servers | none | same |
| IAM DeleteRole / DeleteUser / DeleteAccessKey | `Action=DeleteRole` on `iam.amazonaws.com` | outage | none | n/a |
| KMS ScheduleKeyDeletion | `X-Amz-Target: TrentService.ScheduleKeyDeletion`, `PendingWindowInDays` | recoverable for 7 to 30 days (default 30), `CancelKeyDeletion`; after that, data under the key is gone ([guide](https://docs.aws.amazon.com/kms/latest/developerguide/deleting-keys.html)) | built in | Rewrite `PendingWindowInDays` to 30. |
| KMS DisableKey | `X-Amz-Target: TrentService.DisableKey` | outage of everything encrypted with the key | `EnableKey` | n/a |
| Secrets Manager DeleteSecret | `X-Amz-Target: secretsmanager.DeleteSecret`, `ForceDeleteWithoutRecovery`, `RecoveryWindowInDays` | recoverable for 7 to 30 days (default 30) with `RestoreSecret`; gone with `ForceDeleteWithoutRecovery=true` ([API](https://docs.aws.amazon.com/secretsmanager/latest/apireference/API_DeleteSecret.html)) | built in | Rewrite: drop `ForceDeleteWithoutRecovery`, set 30 days. |
| ECR DeleteRepository / BatchDeleteImage | `X-Amz-Target: AmazonEC2ContainerRegistry_V20150921.DeleteRepository`, `force=true` | gone (images) | none | Images can be rebuilt; no cheap copy. |
| ElastiCache DeleteReplicationGroup / DeleteCacheCluster | `Action=...`, `FinalSnapshotIdentifier` absent | gone | final snapshot only if named | Rewrite: add `FinalSnapshotIdentifier`. |
| CloudFront DeleteDistribution | `DELETE /2020-05-31/distribution/{Id}` with `If-Match` | outage | only a disabled distribution can be deleted, so an `UpdateDistribution` with `Enabled=false` comes first | Hold the disable. |
| EFS DeleteFileSystem | `DELETE /2015-02-01/file-systems/{FileSystemId}` | gone | only an AWS Backup copy | AWS Backup `StartBackupJob`, slow. |
| Redshift DeleteCluster | `Action=DeleteCluster`, `SkipFinalClusterSnapshot=true` | gone | final snapshot | Rewrite. |
| CloudWatch Logs DeleteLogGroup | `X-Amz-Target: Logs_20140328.DeleteLogGroup` | gone | none | n/a |
| AWS Backup DeleteRecoveryPoint / DeleteBackupVault | `DELETE /backup-vaults/{name}/recovery-points/{arn}` | gone, unless the vault is locked | Vault Lock: in compliance mode, after a grace time of at least 3 days, nobody, "including the root user", can delete a backup before its retention ends ([guide](https://docs.aws.amazon.com/aws-backup/latest/devguide/vault-lock.html)) | n/a |
| Organizations CloseAccount | `X-Amz-Target: AWSOrganizationsV20161128.CloseAccount` | recoverable for 90 days through Support; after that AWS deletes even locked backup vaults ([Vault Lock page](https://docs.aws.amazon.com/aws-backup/latest/devguide/vault-lock.html)) | Support | n/a |
| Cost Explorer DeleteCostCategoryDefinition, DeleteAnomalyMonitor | `X-Amz-Target: AWSInsightsIndexService.<Op>` | outage | none | n/a |

The `X-Amz-Target` prefixes in this table come from the botocore models (`metadata.targetPrefix`).
The JSON file has the exact form for every row.

### Destructive parameters on calls that are not deletes

Each of these is in `destructive_params` in the JSON, with its wire name:

- RDS `ModifyDBInstance` / `ModifyDBCluster`: `DeletionProtection=false`, `BackupRetentionPeriod=0`
  (turns automated backups off).
- DynamoDB `UpdateTable`: `DeletionProtectionEnabled=false`. `UpdateContinuousBackups`:
  `PointInTimeRecoveryEnabled=false`.
- EC2 `ModifyInstanceAttribute`: `DisableApiTermination=false`.
- EKS `UpdateClusterConfig`: `deletionProtection=false`.
- CloudFormation `UpdateTerminationProtection`: `EnableTerminationProtection=false`. `UpdateStack`
  with a template that drops a resource. `DeleteStack` with `DeletionMode=FORCE_DELETE_STACK`.
- S3 `PutBucketVersioning` with `Suspended`, `PutBucketLifecycleConfiguration` with an expiration,
  `PutBucketPolicy` that denies the caller, `DeleteObject` with `BypassGovernanceRetention=true`.
- Secrets Manager `DeleteSecret`: `ForceDeleteWithoutRecovery=true`. KMS `ScheduleKeyDeletion`:
  `PendingWindowInDays=7`.
- ECS `UpdateService`: `desiredCount=0`. Auto Scaling `UpdateAutoScalingGroup`:
  `DesiredCapacity=0`. Lambda `PutFunctionConcurrency`: `ReservedConcurrentExecutions=0`.
- ECR `DeleteRepository` and ECS `DeleteService`: `force=true`.

The lesson from the DataTalks.Club incident (section 4) applies here: deletion protection only
helps if turning it off is held too. The author notes that his new protections "can still be
removed via CLI".

### Token scoping on AWS

IAM can deny any action by name, for example `rds:DeleteDBInstance`, and IAM condition keys can
deny some parameters. A user can make a role that cannot delete. The weak point is that an agent
told to "clean up" is given a role that can, which is what happened at DataTalks.Club and,
according to Amazon's own statement, in the Kiro incident ("misconfigured access controls").

## 2. AWS signing: why the placeholder swap fails and what works

### Why a swap cannot work

SigV4 puts an HMAC of the request into the `Authorization` header. The HMAC key is derived from the
secret access key. The secret itself never crosses the wire. A placeholder secret in the computer
produces a signature AWS rejects, and the boundary cannot fix one header value to make it valid.

### The three options

**A. Silo signs on the way out (recommended).** The computer holds placeholder AWS credentials
(`AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`). The agent's SDK signs with them. The boundary ends
TLS for the AWS hosts tied to this signing secret, reads the request, decides allow, hold or deny,
then removes the agent's signature and signs the request again with the real credentials. It
writes a fresh `X-Amz-Date`. The real key never enters the computer.

**B. Short-lived credentials issued by Silo.** Silo calls `sts:AssumeRole` with a session policy
and hands the computer the temporary credentials. A session policy can only remove permissions:
"Session policies limit permissions for a created session, but do not grant permissions"
([IAM guide](https://docs.aws.amazon.com/IAM/latest/UserGuide/access_policies.html)). The SDKs can
fetch credentials from an HTTP endpoint named by `AWS_CONTAINER_CREDENTIALS_FULL_URI`, supported by
every current AWS SDK and the CLI ([SDK reference](https://docs.aws.amazon.com/sdkref/latest/guide/feature-container-credentials.html)),
so Silo could serve them from inside its own network. The limit: the computer holds a real secret.
The agent can sign anything the session allows and can send the credentials to any host it is
allowed to reach. A hold is impossible because the boundary would have no reason to end TLS. Deny
by IAM is all-or-nothing and cannot ask the user.

**C. The agent uses its own keys and Silo only reads.** Silo could end TLS for AWS hosts and hold
or deny a request it reads. But the real key is in the computer, so the agent can copy it to any
host it may reach and use it from outside, where Silo sees nothing. This protects against
mistakes only, not against a hostile agent.

### Is re-signing feasible?

Yes. The boundary must recompute the canonical request from the bytes it forwards. The parts are
the method, the path, the query, the signed headers, and the payload hash in
`x-amz-content-sha256`. The cases:

| `x-amz-content-sha256` | What Silo does |
|---|---|
| hex SHA-256 of the body | Keep the hash. Silo reads the body anyway to inspect it. If Silo rewrites the body, it recomputes the hash. |
| `UNSIGNED-PAYLOAD` | Keep it. The body is not in the signature. botocore uses it for S3 over HTTPS when a checksum header is present (`botocore/auth.py`, `S3SigV4Auth._should_sha256_sign_payload`). |
| `STREAMING-UNSIGNED-PAYLOAD-TRAILER` | Re-sign the headers only. The chunks carry no signature, so the body streams through unchanged. botocore sends this for S3 uploads with a trailing checksum (`botocore/auth.py`). |
| `STREAMING-AWS4-HMAC-SHA256-PAYLOAD` and `...-TRAILER` | Each chunk header carries `chunk-signature=<64 hex>`. Each chunk's string to sign holds the previous signature, the hash of the empty string and the hash of the chunk data; the trailer is signed the same way with `AWS4-HMAC-SHA256-TRAILER` ([S3 trailing headers](https://docs.aws.amazon.com/AmazonS3/latest/developerguide/sigv4-streaming-trailers.md)). Silo rewrites each chunk signature as the chunk passes. The signature has a fixed length, so `Content-Length` does not change and nothing needs buffering. |
| SigV4a (`AWS4-ECDSA-P256-SHA256`, S3 Multi-Region Access Points) | Re-sign with an ECDSA key derived from the real secret. Whether chunked SigV4a signatures keep a fixed length: unverified. |

Other cases:

- **Presigned URLs** carry the signature in the query (`X-Amz-Signature`). Silo re-signs those
  too. A presigned URL made inside the computer works only through Silo, so a URL the agent leaks
  to the outside is useless.
- **Event streams** (Transcribe streaming, Bedrock bidirectional streams) sign each message with
  a chain like the chunked uploads. These are not destructive. Silo can deny them at first.
- **Service rules for the canonical path.** S3 does not double-encode the path, other services do.
  The AWS SDKs already encode these rules. An existing signer library (for example the Rust
  `aws-sigv4` crate from the AWS SDK for Rust) takes them as settings. Silo should reuse one and
  not write its own.

### What re-signing gives Silo beyond the swap

1. **Late approval works.** AWS rejects a request whose `X-Amz-Date` is more than a few minutes
   old. Silo signs at release time, so a request approved late still carries a fresh date.
2. **Silo can rewrite toward undo.** A deny-or-pass boundary can only refuse. A signing boundary
   can let `DeleteDBInstance` pass with `SkipFinalSnapshot=false`, a named final snapshot and
   `DeleteAutomatedBackups=false`. It can strip `ForceDeleteWithoutRecovery`, raise
   `PendingWindowInDays` to 30 and add `FinalSnapshotIdentifier` to ElastiCache and Redshift
   deletes. The provider then makes its own undo, with no extra call and no wait. This changes
   what passes, so it needs a product decision (see "Open questions").
3. **Layers.** Silo can sign with STS credentials it gets from the user's key, adding a session
   policy that denies what the user never allows. The boundary rule and IAM then both refuse it.

### The SDK timeout and the 90-second hold

botocore's default read timeout is 60 seconds (`DEFAULT_TIMEOUT = 60` in `botocore/endpoint.py`).
The default retry mode in this botocore is `standard` (`retry_mode` default in
`botocore/configprovider.py`): 3 attempts in all, and a read timeout counts as a retryable
error (`ReadTimeoutError` is an `HTTPClientError`, `botocore/retries/standard.py`). The `legacy`
mode allows 5 attempts (10 for DynamoDB, `botocore/data/_retry.json`). A request held for
90 seconds therefore times out in the SDK first. The SDK then sends the same request again, and the
boundary sees a second held request for the same operation. Silo must merge repeated identical
requests into one ask. On a yes it releases one copy and answers the others. Many AWS operations
carry an idempotency token (`ClientRequestToken`) that makes the merge exact. Whether the AWS CLI v2
and the Terraform AWS provider use the same 60-second default: unverified.

### The most robust design

Option A with option B's policy as a second layer. Silo holds the long-lived AWS key, or better a
role it can assume. The computer gets placeholder credentials. Silo signs every request on its
way out, and the boundary decides on the operation it read. For an allowed delete with a known
undo, Silo either rewrites the request toward the provider's own undo or makes a snapshot call
first.

## 3. AWS: snapshot before the delete, and how long it takes

| Silo's call | Duration | Fits in a 90 s hold? |
|---|---|---|
| RDS `CreateDBSnapshot` | Depends on size. On a Single-AZ instance it suspends I/O briefly, "from a few seconds to a few minutes" ([guide](https://docs.aws.amazon.com/AmazonRDS/latest/UserGuide/USER_CreateSnapshot.html)) | No. The rewrite to a final snapshot is better, because RDS waits on its own. |
| EBS `CreateSnapshot` / `CreateSnapshots` | Point-in-time at the request, `pending` for up to "several hours" ([guide](https://docs.aws.amazon.com/ebs/latest/userguide/ebs-creating-snapshot.html)) | The capture is immediate. Whether the delete can go on while the snapshot is pending: unverified. |
| DynamoDB `CreateBackup` | "processed instantaneously", available "within minutes"; the table cannot be deleted while a backup is in progress ([guide](https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/CreateBackup.html)) | Usually, unverified for large tables. |
| S3 `PutBucketVersioning` Enabled | One request | Yes. |

So the hold that covers "make it undoable" must be separate from the hold that waits for the user.
Silo makes its call, waits for the provider to report the copy ready, and only then lets the
delete pass. A rule may need a hold longer than 90 seconds for this. Late approval from the phone
(designed elsewhere) covers the case where the agent's connection is gone.

## 4. Terraform, CDK and Pulumi on the wire

**`terraform destroy`** reads every resource (Describe/Get calls), then deletes in reverse
dependency order. It runs up to 10 operations at once: `-parallelism=n` "Limit the number of
concurrent operations as Terraform walks the graph", default 10
([Terraform docs](https://developer.hashicorp.com/terraform/cli/commands/apply)). Each delete is its
own API call, followed by Describe polling until the resource is gone. So on the wire a destroy is
a burst of up to 10 deletes at once, then more as dependencies clear (instances, then subnets, then
the VPC). A held delete stalls only its own branch. Other branches go on deleting.

**`terraform apply` with a replacement** (`-/+` in the plan) deletes and then creates, or creates
first with `create_before_destroy`. The delete looks the same as in a destroy.

What the Terraform AWS provider sends depends on the configuration. `aws_db_instance` has
`skip_final_snapshot` default `false` and `delete_automated_backups` default `true`, and
`deletion_protection` defaults to `false`. The provider's own examples set
`skip_final_snapshot = true`. `aws_s3_bucket` with `force_destroy = true` deletes every object,
"including any locked objects". `aws_secretsmanager_secret` with `recovery_window_in_days = 0`
deletes the secret without recovery. Sources: the provider docs in
`website/docs/r/{db_instance,s3_bucket,secretsmanager_secret}.html.markdown` at
`github.com/hashicorp/terraform-provider-aws`, main branch.

**HCP Terraform (Terraform Cloud) API.** A run is created with `POST /api/v2/runs`, with
`is-destroy` for a destroy and `auto-apply` from the workspace. A run that is not auto-applied
waits until `POST /runs/:run_id/actions/apply` or `/actions/discard`
([Runs API](https://developer.hashicorp.com/terraform/cloud-docs/api-docs/run)). The plan is
readable at `GET /plans/:id/json-output` or `GET /runs/:id/plan/json-output`, with
`resource-destructions` as a count ([Plans API](https://developer.hashicorp.com/terraform/cloud-docs/api-docs/plans)).
For Silo this is the best place to hold. One ask on the apply call can show the plan's
destructions, read by Silo from the plan JSON, in place of a storm of asks for each delete. For
local `terraform`, no such point exists on the wire.

**`cdk destroy`** is one `Action=DeleteStack` call plus polling. CloudFormation deletes the
resources on the AWS side, so Silo sees one call. The data outcome is set by each resource's
`DeletionPolicy`. A `cdk deploy` that removes resources goes through `CreateChangeSet` and
`ExecuteChangeSet`. Silo can read the change set and show what it removes (section 1).

**`pulumi destroy`** with the classic AWS provider calls the AWS APIs directly, like Terraform.
With AWS Native it calls Cloud Control `DeleteResource`. Neither was observed on the wire:
unverified.

### DataTalks.Club (February 2026): what was lost and what came back

From the founder's own write-up ([post](https://aishippingblog.com/p/how-i-dropped-our-production-database),
formerly on alexeyondata.substack.com):

- The Terraform state file was on another computer. Claude Code ran `terraform plan` and
  `terraform apply`, which started creating duplicates. The agent removed duplicates with the AWS
  CLI. It then "replaced my current state file with an older one" that described production, and
  ran `terraform destroy`.
- Lost: the VPC, the RDS database, the ECS cluster, the load balancers and the bastion host. The
  automated backups were deleted with the database. Coverage speaks of 2.5 years of course data
  ([Tom's Hardware](https://www.tomshardware.com/tech-industry/artificial-intelligence/claude-code-deletes-developers-production-setup-including-its-database-and-snapshots-2-5-years-of-records-were-nuked-in-an-instant)).
- Recovery: he bought AWS Business Support. Support found "a snapshot exists on their side" that
  his console did not show, and restored it "Exactly 24 hours after the database had been
  deleted". This matches the RDS guide's line that Support "might" recover a deleted instance "for
  up to six days". The table he names had 1,943,200 rows after the restore.
- His changes: backups outside Terraform, a nightly restore test, deletion protection in Terraform
  and AWS, versioned backup buckets, remote state in S3, and agents no longer run Terraform.

For Silo: the whole loss went through RDS `DeleteDBInstance` with backups deleted. A hold on that
one call, or a rewrite to keep automated backups, would have saved it. The CLI deletes before the
destroy were also through the same credentials.

### Kiro (December 2025)

The Financial Times reported, from anonymous sources, that Amazon's Kiro agent decided to "delete
and recreate the environment", causing a 13-hour disruption of AWS Cost Explorer in one region of
mainland China. Amazon said the event was "extremely limited" and caused by "user error,
specifically misconfigured access controls" ([The Register, 2026-02-20](https://www.theregister.com/2026/02/20/amazon_denies_kiro_agentic_ai_behind_outage/)).
Which API calls were made: unverified. This was inside AWS's own systems, so it shows the pattern
(an agent with an operator's permissions) rather than a customer-reachable call.

## 5. Google Cloud

### How the operation shows on the wire

gcloud and most client libraries send REST JSON, for example
`DELETE https://sqladmin.googleapis.com/v1/projects/{project}/instances/{instance}`. The method and
the path give the operation, and the discovery document maps every pair to a method name. Some
client libraries use gRPC over HTTP/2, where the operation is the `:path` pseudo-header, of the form
`/google.<api>.<version>.<Service>/<Method>`. Which libraries pick gRPC by default: unverified.
Inspecting gRPC parameters needs the API's protobuf descriptors, while the method name needs only
the path.

### Signing is not a blocker on Google Cloud

Google APIs take `Authorization: Bearer <access token>`. That is a value the boundary can swap,
the same as an API key. The signing step happens earlier: a service-account key signs a JWT that
`oauth2.googleapis.com` exchanges for an access token. Silo can do that exchange itself and give
the computer a placeholder token. Client libraries also fetch tokens from the metadata server
(`metadata.google.internal`, header `Metadata-Flavor: Google`), which Silo could answer with a
placeholder. That the libraries honour a `GCE_METADATA_HOST` override: unverified.

### Operations

The paths come from the discovery documents. The parameters and fields named are the ones in
those documents.

| Operation | On the wire | Outcome | Provider-side undo | Undo by Silo |
|---|---|---|---|---|
| Cloud SQL instances.delete | `DELETE sqladmin.googleapis.com/v1/projects/{project}/instances/{instance}`, query `enableFinalBackup`, `finalBackupTtlDays` | gone by default | "Cloud SQL deletes all resources for the instance, including on-demand backups and automated backups" ([delete](https://docs.cloud.google.com/sql/docs/mysql/delete-instance)). `enableFinalBackup` is off by default ([API](https://docs.cloud.google.com/sql/docs/mysql/admin-api/rest/v1/instances/delete)). The instance setting `retainBackupsOnDelete` keeps backups. A final backup lasts 1 to 365 days, 30 by default. Customer Care may recover an instance deleted without a final backup "within 4 days". Refused while `settings.deletionProtectionEnabled`. | Yes, by rewrite: add `enableFinalBackup=true`. Here the token is swapped, not signed, so Silo can change the query. |
| Cloud SQL instances.patch | `PATCH .../instances/{instance}` with `deletionProtectionEnabled=false` or backups off | prepares gone | n/a | Hold it. |
| Cloud Storage objects.delete | `DELETE storage.googleapis.com/storage/v1/b/{bucket}/o/{object}` | recoverable | Soft delete is on by default for 7 days, settable from 7 to 90, or 0 to turn it off ([soft delete](https://docs.cloud.google.com/storage/docs/soft-delete)). `objects.restore` brings it back. | none needed while soft delete is on |
| Cloud Storage buckets.delete | `DELETE .../b/{bucket}` | recoverable | A deleted bucket is soft-deleted too, and `buckets.restore` brings back the empty bucket; objects are restored separately. | n/a |
| Cloud Storage buckets.patch | `softDeletePolicy.retentionDurationSeconds=0` | prepares gone | n/a | Hold it. |
| Project delete | `DELETE cloudresourcemanager.googleapis.com/v3/projects/{project}` | outage at once, recoverable for 30 days | Shutdown "stops all billing and resource usage"; `POST .../projects/{project}:undelete` restores within 30 days, but billing must be relinked, Cloud Storage data protected by soft delete "may be removed after 7 days" and Pub/Sub sooner ([delete and restore](https://docs.cloud.google.com/resource-manager/docs/delete-restore-projects)). A lien (`POST /v3/liens`) blocks deletion. | Silo can place a lien. |
| Compute instances.delete | `DELETE compute.googleapis.com/compute/v1/projects/{p}/zones/{z}/instances/{i}` | gone for boot disks with auto-delete | Refused while `deletionProtection` is set (`instances.setDeletionProtection`) | Snapshot disks first (`POST .../disks/{d}/createSnapshot`, not timed here). |
| Compute disks.delete, regionDisks.delete | `DELETE .../zones/{z}/disks/{d}` | gone | The v1 `Disk` resource has no deletion protection field (discovery). A Compute recycle bin: unverified. | Snapshot first |
| Compute snapshots.delete, images.delete | `DELETE .../global/snapshots/{s}` | gone | none found | n/a |
| GKE clusters.delete | `DELETE container.googleapis.com/v1/projects/{p}/locations/{l}/clusters/{c}` | outage, and Kubernetes state is gone | No deletion protection field in the v1 API (discovery). Terraform's `google_container_cluster` has its own `deletion_protection` check, on the client side only. | Backup for GKE, if set up |
| BigQuery datasets.delete | `DELETE bigquery.googleapis.com/bigquery/v2/projects/{p}/datasets/{d}`, `deleteContents=true` | recoverable | Time travel window 2 to 7 days, 7 by default (`maxTimeTravelHours`), then 7 more days of fail-safe that needs Customer Care ([time travel](https://docs.cloud.google.com/bigquery/docs/time-travel)). `POST .../datasets/{d}:undelete` restores a dataset "within time travel window" (discovery). | none needed |
| BigQuery tables.delete | `DELETE .../datasets/{d}/tables/{t}` | recoverable | same window | none needed |
| Firestore databases.delete | `DELETE firestore.googleapis.com/v1/projects/{p}/databases/{db}` | gone, unverified | Delete protection (`deleteProtectionState`) is "disabled by default" ([manage databases](https://docs.cloud.google.com/firestore/native/docs/manage-databases)). PITR keeps 7 days of versions, otherwise 1 hour (`versionRetentionPeriod`). Whether backups or PITR data survive a database delete: unverified. | `projects.databases.exportDocuments` or a backup first, not timed here |
| Firestore documents.delete | `DELETE .../databases/{db}/documents/{path}` | recoverable with PITR for 7 days, by clone or read at a past time | | |
| Cloud Run services.delete, jobs.delete | `DELETE run.googleapis.com/v2/projects/{p}/locations/{l}/services/{s}` | outage (the image stays in the registry) | No deletion protection field in v2 (discovery) | Silo can save the service spec (`GET`) to redeploy it. |

## 6. Azure, briefly

Azure Resource Manager calls are REST on `management.azure.com`, for example
`DELETE /subscriptions/{sub}/resourceGroups/{rg}?api-version=...`. A resource group delete removes
everything in it. Authentication is `Authorization: Bearer <Entra token>`, which the boundary can
swap like any other token.

- **Locks.** `CanNotDelete` and `ReadOnly` locks apply to a subscription, a resource group or a
  resource, and are inherited. A delete lock on one resource blocks the delete of its whole
  resource group. Locks cover only the control plane at `management.azure.com`. A lock on a storage
  account "doesn't protect its data from being deleted", because blob deletes go to
  `<account>.blob.core.windows.net`. Removing a lock needs `Microsoft.Authorization/locks/*`, held
  by Owner and User Access Administrator ([locks](https://learn.microsoft.com/en-us/azure/azure-resource-manager/management/lock-resources)).
  Silo can hold `DELETE .../providers/Microsoft.Authorization/locks/{name}` and place a lock itself
  with `PUT` on the same path.
- **Key Vault.** Soft delete is on by default for new vaults and cannot be turned off. It keeps
  vaults and objects 7 to 90 days, 90 by default. Purge protection is off by default. A purge is
  immediate and cannot be undone ([soft delete](https://learn.microsoft.com/en-us/azure/key-vault/general/soft-delete-overview)).
- Blob soft delete, Azure SQL deleted-database restore and Azure Backup were not checked here:
  unverified.

## 7. What this means for Silo

1. **For AWS, the boundary must sign.** Swapping cannot work for SigV4, and short-lived
   credentials in the computer cannot be held. Signing at the boundary is feasible for every
   payload mode the SDKs use, including chunked uploads, without buffering bodies.
2. **The operation is always a fact on the wire, but in four places.** These are the form body
   (`Action`), a header (`X-Amz-Target`), the method plus path, and a CBOR path. A service can
   speak several, and Cloud Control and CloudFormation delete many resource types through one
   generic call. The JSON file gives the matcher table, derived from the same models the SDKs use,
   so it can be regenerated for each botocore release.
3. **The most valuable AWS rule is a short list.** RDS `DeleteDBInstance`/`DeleteDBCluster`, S3
   `DeleteObjects`/`DeleteObject` on buckets without versioning, DynamoDB `DeleteTable`, EC2
   `TerminateInstances`/`DeleteVolume`, CloudFormation `DeleteStack`/`ExecuteChangeSet`, and every
   call that turns a protection off. The protections only work if turning them off is held too.
4. **Silo can often make a delete undoable at no cost.** On AWS this works by rewriting the
   request toward the provider's own undo (final snapshot, retained backups, longest recovery
   window). On Google Cloud it works through `enableFinalBackup` and liens. On S3 it works by
   turning versioning on first. Copies made by Silo with its own call (RDS and EBS snapshots) take
   minutes to hours and do not fit a 90-second hold.
5. **Infrastructure tools need one ask per plan.** HCP Terraform exposes the plan and a separate
   apply call, and CloudFormation exposes change sets. Local `terraform destroy` reaches the wire
   as a burst of up to 10 concurrent deletes, each a separate held request. Silo should group them
   into one ask by credential and time window.

## Open questions for the owner

- **May a precaution rewrite a request?** The brief says opinions may only "protect
  (deny/hold/pause)". A rewrite that turns `SkipFinalSnapshot=true` into a final snapshot protects,
  but it is neither deny nor hold, and the agent gets a different result than it asked for. It is
  also not an opinion: the rewrite follows from facts. I propose a fourth action next to allow,
  hold and deny, written by the user's rule and shown in the ask: "let it pass, made undoable".
- **Hold length for undo.** Waiting for a DynamoDB backup or an RDS snapshot before releasing a
  delete can take longer than 90 seconds, and the SDK times out at 60 seconds and retries. The hold
  needs to merge retries, and the rule needs a length that covers the copy.
- **Is turning S3 versioning on, which cannot be undone, acceptable as Silo's own call?** It is the
  cheapest undo for S3, but it changes the bucket for good and adds storage cost.

## Not verified

- Pulumi AWS Native using Cloud Control on the wire.
- The Kiro incident's actual calls.
- Whether an EBS volume can be deleted while its snapshot is pending.
- Whether chunked SigV4a signatures keep a fixed length.
- Default timeouts of the AWS CLI v2 and the Terraform AWS provider.
- Which Google client libraries use gRPC by default, and the `GCE_METADATA_HOST` override.
- A Compute Engine recycle bin, and whether Firestore backups survive a database delete.
- Azure blob soft delete and SQL deleted-database restore.
