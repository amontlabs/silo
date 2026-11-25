# The CLI captures, command by command

Each command of `proto/capture/commands.tsv`, its label, each write it sent (reads are left out)
and the decision of the fallback alone, the blind rules, the blind rules with the fallback, and the
tuned rules. **hold** is a hold; a dash is allow. A mark after a decision: `!` it disagrees with the label.

## aws

| Command | Label | Write | fallback | blind | blind+fb | tuned |
|---|---|---|---|---|---|---|
| `aws-s3-rb-force` | hold:gone | `DELETE s3/b1/b.csv` | **hold** | **hold** | **hold** | **hold** |
| `aws-s3-rb-force` | hold:gone | `DELETE s3/b1/a.csv` | **hold** | **hold** | **hold** | **hold** |
| `aws-s3-rb-force` | hold:gone | `DELETE s3/b1` | **hold** | **hold** | **hold** | **hold** |
| `aws-s3-rm-recursive` | hold:gone | `DELETE s3/b1/logs/a.csv` | **hold** | **hold** | **hold** | **hold** |
| `aws-s3-rm-recursive` | hold:gone | `DELETE s3/b1/logs/b.csv` | **hold** | **hold** | **hold** | **hold** |
| `aws-s3-rm` | hold:gone | `DELETE s3/b1/report.csv` | **hold** | **hold** | **hold** | **hold** |
| `aws-s3-sync-delete` | hold:gone | `DELETE s3/b1/a.csv` | **hold** | **hold** | **hold** | **hold** |
| `aws-s3-sync-delete` | hold:gone | `DELETE s3/b1/b.csv` | **hold** | **hold** | **hold** | **hold** |
| `aws-s3-cp` | either | `PUT s3/b1/hostname` | - | - | - | - |
| `aws-s3-mb` | allow | `PUT s3/b2` | - | - | - | - |
| `aws-s3-ls` | allow | (a read only) | | | | |
| `aws-s3api-policy-public` | hold:exposed | `PUT s3/b1` | **hold** | **hold** | **hold** | **hold** |
| `aws-s3api-acl-public` | hold:exposed | `PUT s3/b1` | - ! | **hold** | **hold** | **hold** |
| `aws-s3api-pab-delete` | hold:exposed | `DELETE s3/b1` | **hold** | **hold** | **hold** | **hold** |
| `aws-s3api-pab-off` | hold:exposed | `PUT s3/b1` | - ! | **hold** | **hold** | **hold** |
| `aws-s3api-versioning-off` | hold:outage | `PUT s3/b1` | - ! | **hold** | **hold** | **hold** |
| `aws-s3api-tagging` | allow | `PUT s3/b1` | - | - | - | - |
| `aws-s3api-delete-objects` | hold:gone | `POST s3/b1` | - ! | **hold** | **hold** | **hold** |
| `aws-ec2-terminate` | hold:gone | `POST ec2/` | **hold** | **hold** | **hold** | **hold** |
| `aws-ec2-stop` | hold:outage | `POST ec2/` | **hold** | **hold** | **hold** | **hold** |
| `aws-ec2-start` | allow | `POST ec2/` | - | - | - | - |
| `aws-ec2-delete-volume` | hold:gone | `POST ec2/` | **hold** | **hold** | **hold** | **hold** |
| `aws-ec2-delete-snapshot` | hold:gone | `POST ec2/` | **hold** | **hold** | **hold** | **hold** |
| `aws-ec2-sg-open` | hold:exposed | `POST ec2/` | **hold** | **hold** | **hold** | **hold** |
| `aws-ec2-sg-private` | either | `POST ec2/` | **hold** | - | - | - |
| `aws-ec2-snapshot-public` | hold:exposed | `POST ec2/` | - ! | **hold** | **hold** | **hold** |
| `aws-ec2-no-termination-protection` | hold:outage | `POST ec2/` | - ! | **hold** | **hold** | **hold** |
| `aws-ec2-create-tags` | allow | `POST ec2/` | - | - | - | - |
| `aws-ec2-describe` | allow | (a read only) | | | | |
| `aws-rds-delete-skip` | hold:gone | `POST rds/` | **hold** | **hold** | **hold** | **hold** |
| `aws-rds-delete-snapshot` | hold:recoverable | `POST rds/` | **hold** | **hold** | **hold** | **hold** |
| `aws-rds-no-protection` | hold:outage | `POST rds/` | **hold** | **hold** | **hold** | **hold** |
| `aws-rds-public` | hold:exposed | `POST rds/` | - ! | **hold** | **hold** | **hold** |
| `aws-rds-class` | allow | `POST rds/` | - | - | - | - |
| `aws-rds-stop` | hold:outage | `POST rds/` | **hold** | **hold** | **hold** | **hold** |
| `aws-rds-reboot` | either | `POST rds/` | - | - | - | - |
| `aws-ddb-delete-table` | hold:gone | `POST dynamodb/` | **hold** | **hold** | **hold** | **hold** |
| `aws-ddb-delete-item` | hold:gone | `POST dynamodb/` | **hold** | **hold** | **hold** | **hold** |
| `aws-ddb-put-item` | either | `POST dynamodb/` | - | - | - | - |
| `aws-ddb-pitr-off` | hold:gone | `POST dynamodb/` | **hold** | **hold** | **hold** | **hold** |
| `aws-ddb-partiql-delete` | hold:gone | `POST dynamodb/` | - ! | **hold** | **hold** | **hold** |
| `aws-lambda-delete` | hold:outage | `DELETE lambda/2015-03-31/functions/f1` | **hold** | **hold** | **hold** | **hold** |
| `aws-lambda-url-public` | hold:exposed | `POST lambda/2021-10-31/functions/f1/url` | - ! | **hold** | **hold** | **hold** |
| `aws-lambda-update-code` | deploy | `PUT lambda/2015-03-31/functions/f1/code` | - | - | - | - |
| `aws-lambda-concurrency-zero` | hold:outage | `PUT lambda/2017-10-31/functions/f1/concurrency` | - ! | **hold** | **hold** | **hold** |
| `aws-iam-delete-user` | hold:outage | `POST iam/` | **hold** | **hold** | **hold** | **hold** |
| `aws-iam-create-key` | hold:exposed | `POST iam/` | **hold** | **hold** | **hold** | **hold** |
| `aws-iam-attach-admin` | hold:exposed | `POST iam/` | - ! | **hold** | **hold** | **hold** |
| `aws-iam-deactivate-key` | hold:outage | `POST iam/` | **hold** | **hold** | **hold** | **hold** |
| `aws-iam-list-users` | allow | (a read only) | | | | |
| `aws-secrets-delete-force` | hold:gone | `POST secretsmanager/` | **hold** | **hold** | **hold** | **hold** |
| `aws-secrets-delete` | hold:recoverable | `POST secretsmanager/` | **hold** | **hold** | **hold** | **hold** |
| `aws-secrets-put` | either | `POST secretsmanager/` | - | - | - | - |
| `aws-kms-schedule-delete` | hold:recoverable | `POST kms/` | - ! | **hold** | **hold** | **hold** |
| `aws-kms-disable` | hold:outage | `POST kms/` | **hold** | **hold** | **hold** | **hold** |
| `aws-r53-delete-record` | hold:outage | `POST route53/2013-04-01/hostedzone/Z1/rrset/` | - ! | **hold** | **hold** | **hold** |
| `aws-r53-upsert` | either | `POST route53/2013-04-01/hostedzone/Z1/rrset/` | - | - | - | - |
| `aws-cfn-delete-stack` | hold:gone | `POST cloudformation/` | **hold** | **hold** | **hold** | **hold** |
| `aws-ecs-scale-zero` | hold:outage | `POST ecs/` | **hold** | **hold** | **hold** | **hold** |
| `aws-ecs-delete-service` | hold:outage | `POST ecs/` | **hold** | **hold** | **hold** | **hold** |
| `aws-eks-delete-cluster` | hold:gone | `DELETE eks/clusters/c1` | **hold** | **hold** | **hold** | **hold** |
| `aws-sqs-purge` | hold:gone | `POST queue/` | **hold** | **hold** | **hold** | **hold** |
| `aws-sqs-send` | allow | `POST queue/` | - | - | - | - |
| `aws-sns-delete-topic` | hold:outage | `POST sns/` | **hold** | **hold** | **hold** | **hold** |
| `aws-logs-delete-group` | hold:gone | `POST logs/` | **hold** | **hold** | **hold** | **hold** |
| `aws-ssm-delete-param` | hold:gone | `POST ssm/` | **hold** | **hold** | **hold** | **hold** |
| `aws-ssm-put-param` | either | `POST ssm/` | - | - | - | - |
| `aws-ecr-delete-repo` | hold:gone | `POST api/` | **hold** | **hold** | **hold** | **hold** |
| `aws-ecr-delete-image` | hold:gone | `POST api/` | **hold** | **hold** | **hold** | **hold** |
| `aws-elasticache-delete` | hold:gone | `POST elasticache/` | **hold** | **hold** | **hold** | **hold** |
| `aws-backup-delete-rp` | hold:gone | `DELETE backup/backup-vaults/v1/recovery-points/arn%3Aaws%3Abackup%3Aus-east-1%3A111` | **hold** | **hold** | **hold** | **hold** |
| `aws-opensearch-delete` | hold:gone | `DELETE es/2021-01-01/opensearch/domain/search1` | **hold** | **hold** | **hold** | **hold** |
| `aws-apprunner-delete` | hold:outage | `POST apprunner/` | **hold** | **hold** | **hold** | **hold** |
| `aws-efs-delete` | hold:gone | `DELETE elasticfilesystem/2015-02-01/file-systems/fs-0abc1234` | **hold** | **hold** | **hold** | **hold** |
| `aws-sts-identity` | allow | (a read only) | | | | |

## gcloud

| Command | Label | Write | fallback | blind | blind+fb | tuned |
|---|---|---|---|---|---|---|
| `gc-vm-delete` | hold:gone | `DELETE compute/compute/v1/projects/proj1/zones/us-central1-a/instances/vm1` | **hold** | **hold** | **hold** | **hold** |
| `gc-vm-stop` | hold:outage | `POST compute/compute/v1/projects/proj1/zones/us-central1-a/instances/vm1/stop` | **hold** | **hold** | **hold** | **hold** |
| `gc-vm-start` | allow | `POST compute/compute/v1/projects/proj1/zones/us-central1-a/instances/vm1/start` | - | - | - | - |
| `gc-vm-no-protection` | hold:outage | `POST compute/compute/v1/projects/proj1/zones/us-central1-a/instances/vm1/setDeleti` | - ! | **hold** | **hold** | **hold** |
| `gc-vm-ssh-key` | hold:exposed | `POST compute/compute/v1/projects/proj1/zones/us-central1-a/instances/vm1/setMetada` | - ! | **hold** | **hold** | **hold** |
| `gc-vm-label` | allow | `POST compute/compute/v1/projects/proj1/zones/us-central1-a/instances/vm1/setLabels` | - | - | - | - |
| `gc-disk-delete` | hold:gone | `DELETE compute/compute/v1/projects/proj1/zones/us-central1-a/disks/d1` | **hold** | **hold** | **hold** | **hold** |
| `gc-snapshot-delete` | hold:gone | `DELETE compute/compute/v1/projects/proj1/global/snapshots/s1` | **hold** | **hold** | **hold** | **hold** |
| `gc-fw-open` | hold:exposed | `POST compute/compute/v1/projects/proj1/global/firewalls` | **hold** | **hold** | **hold** | **hold** |
| `gc-fw-private` | either | `POST compute/compute/v1/projects/proj1/global/firewalls` | **hold** | - | - | - |
| `gc-mig-zero` | hold:outage | `POST compute/compute/v1/projects/proj1/zones/us-central1-a/instanceGroupManagers/m` | - ! | **hold** | **hold** | **hold** |
| `gc-address-delete` | hold:outage | `DELETE compute/compute/v1/projects/proj1/regions/us-central1/addresses/ip1` | **hold** | **hold** | **hold** | **hold** |
| `gc-vm-list` | allow | (a read only) | | | | |
| `gc-gcs-rm` | hold:gone | `DELETE storage/storage/v1/b/b1/o/report.csv` | **hold** | **hold** | **hold** | **hold** |
| `gc-gcs-rm-r` | hold:gone | `DELETE storage/storage/v1/b/b1/o/a.csv` | **hold** | **hold** | **hold** | **hold** |
| `gc-gcs-rm-r` | hold:gone | `DELETE storage/storage/v1/b/b1/o/b.csv` | **hold** | **hold** | **hold** | **hold** |
| `gc-gcs-rm-r` | hold:gone | `DELETE storage/storage/v1/b/b1` | **hold** | **hold** | **hold** | **hold** |
| `gc-gcs-bucket-delete` | hold:outage | `DELETE storage/storage/v1/b/b1` | **hold** | **hold** | **hold** | **hold** |
| `gc-gcs-no-versioning` | hold:outage | `PATCH storage/storage/v1/b/b1` | - ! | **hold** | **hold** | **hold** |
| `gc-gcs-public` | hold:exposed | `PUT storage/storage/v1/b/b1/iam` | - ! | - ! | - ! | **hold** |
| `gc-gcs-user` | either | `PUT storage/storage/v1/b/b1/iam` | - | - | - | - |
| `gc-gcs-cp` | either | `POST storage/upload/storage/v1/b/b1/o` | - | - | - | - |
| `gc-gcs-cp` | either | `DELETE storage/storage/v1/b/b1/o/hostname` | **hold** | **hold** | **hold** | **hold** |
| `gc-sql-delete` | hold:gone | `DELETE sqladmin/sql/v1beta4/projects/proj1/instances/db1` | **hold** | **hold** | **hold** | **hold** |
| `gc-sql-no-protection` | hold:outage | `PATCH sqladmin/sql/v1beta4/projects/proj1/instances/db1` | **hold** | **hold** | **hold** | **hold** |
| `gc-sql-open` | hold:exposed | `PATCH sqladmin/sql/v1beta4/projects/proj1/instances/db1` | - ! | **hold** | **hold** | **hold** |
| `gc-sql-stop` | hold:outage | `PATCH sqladmin/sql/v1beta4/projects/proj1/instances/db1` | - ! | **hold** | **hold** | **hold** |
| `gc-sql-db-delete` | hold:gone | `DELETE sqladmin/sql/v1beta4/projects/proj1/instances/db1/databases/app` | **hold** | **hold** | **hold** | **hold** |
| `gc-sql-user-delete` | hold:outage | `DELETE sqladmin/sql/v1beta4/projects/proj1/instances/db1/users` | **hold** | **hold** | **hold** | **hold** |
| `gc-sql-backup-delete` | hold:gone | `DELETE sqladmin/sql/v1beta4/projects/proj1/instances/db1/backupRuns/123` | **hold** | **hold** | **hold** | **hold** |
| `gc-run-delete` | hold:outage | `DELETE us-central1-run/apis/serving.knative.dev/v1/namespaces/proj1/services/web` | **hold** | **hold** | **hold** | **hold** |
| `gc-run-public` | hold:exposed | `POST run/v1/projects/proj1/locations/us-central1/services/web:setIamPolicy` | - ! | **hold** | **hold** | **hold** |
| `gc-run-deploy` | deploy | `PUT us-central1-run/apis/serving.knative.dev/v1/namespaces/proj1/services/web` | - | - | - | - |
| `gc-fn-delete` | hold:outage | `DELETE cloudfunctions/v1/projects/proj1/locations/us-central1/functions/f1` | **hold** | - ! | **hold** | **hold** |
| `gc-gke-delete` | hold:gone | `DELETE container/v1/projects/proj1/locations/us-central1-a/clusters/c1` | **hold** | **hold** | **hold** | **hold** |
| `gc-gke-zero` | hold:outage | `POST container/v1/projects/proj1/locations/us-central1-a/clusters/c1/nodePools/defau` | - ! | **hold** | **hold** | **hold** |
| `gc-secret-delete` | hold:gone | `DELETE secretmanager/v1/projects/proj1/secrets/s1` | **hold** | **hold** | **hold** | **hold** |
| `gc-secret-destroy` | hold:gone | `POST secretmanager/v1/projects/proj1/secrets/s1/versions/1:destroy` | **hold** | **hold** | **hold** | **hold** |
| `gc-secret-add` | either | `POST secretmanager/v1/projects/proj1/secrets/s1:addVersion` | - | - | - | - |
| `gc-kms-destroy` | hold:recoverable | `POST cloudkms/v1/projects/proj1/locations/global/keyRings/r1/cryptoKeys/k1/cryptoKe` | **hold** | **hold** | **hold** | **hold** |
| `gc-project-delete` | hold:recoverable | `DELETE cloudresourcemanager/v1/projects/proj1` | **hold** | - ! | **hold** | **hold** |
| `gc-project-owner` | hold:exposed | `POST cloudresourcemanager/v1/projects/proj1:setIamPolicy` | - ! | - ! | - ! | - ! |
| `gc-sa-key` | hold:exposed | `POST iam/v1/projects/-/serviceAccounts/sa1@proj1.iam.gserviceaccount.com/keys` | **hold** | **hold** | **hold** | **hold** |
| `gc-sa-delete` | hold:outage | `DELETE iam/v1/projects/-/serviceAccounts/sa1@proj1.iam.gserviceaccount.com` | **hold** | **hold** | **hold** | **hold** |
| `gc-pubsub-delete` | hold:outage | `DELETE pubsub/v1/projects/proj1/topics/t1` | **hold** | **hold** | **hold** | **hold** |
| `gc-dns-delete` | hold:outage | `DELETE dns/dns/v1/projects/proj1/managedZones/z1/rrsets/api.example.com./A` | **hold** | **hold** | **hold** | **hold** |
| `gc-log-delete` | hold:gone | `DELETE logging/v2/projects/proj1/logs/l1` | **hold** | **hold** | **hold** | **hold** |
| `gc-ar-delete` | hold:gone | `DELETE artifactregistry/v1/projects/proj1/locations/us/repositories/r1` | **hold** | **hold** | **hold** | **hold** |
| `gc-firestore-delete` | hold:gone | `DELETE firestore/v1/projects/proj1/databases/d1` | **hold** | **hold** | **hold** | **hold** |
| `gc-bq-rm` | hold:gone | `DELETE bigquery/bigquery/v2/projects/proj1/datasets/ds/tables/users` | **hold** | **hold** | **hold** | **hold** |
| `gc-bq-drop` | hold:gone | `POST bigquery/bigquery/v2/projects/proj1/jobs` | - ! | **hold** | **hold** | **hold** |
| `gc-bq-select` | allow | `POST bigquery/bigquery/v2/projects/proj1/jobs` | - | - | - | - |

## az

| Command | Label | Write | fallback | blind | blind+fb | tuned |
|---|---|---|---|---|---|---|
| `az-group-delete` | hold:gone | `DELETE management/subscriptions/00000000-0000-0000-0000-00000000000a/resourcegroups/rg1` | **hold** | **hold** | **hold** | **hold** |
| `az-vm-delete` | hold:outage | `DELETE management/subscriptions/00000000-0000-0000-0000-00000000000a/resourceGroups/rg1` | **hold** | **hold** | **hold** | **hold** |
| `az-vm-deallocate` | hold:outage | `POST management/subscriptions/00000000-0000-0000-0000-00000000000a/resourceGroups/rg1` | **hold** | **hold** | **hold** | **hold** |
| `az-vm-start` | allow | `POST management/subscriptions/00000000-0000-0000-0000-00000000000a/resourceGroups/rg1` | - | - | - | - |
| `az-disk-delete` | hold:gone | `DELETE management/subscriptions/00000000-0000-0000-0000-00000000000a/resourceGroups/rg1` | **hold** | **hold** | **hold** | **hold** |
| `az-snapshot-delete` | hold:gone | `DELETE management/subscriptions/00000000-0000-0000-0000-00000000000a/resourceGroups/rg1` | **hold** | **hold** | **hold** | **hold** |
| `az-sa-delete` | hold:gone | `DELETE management/subscriptions/00000000-0000-0000-0000-00000000000a/resourceGroups/rg1` | **hold** | **hold** | **hold** | **hold** |
| `az-sa-public` | hold:exposed | `PATCH management/subscriptions/00000000-0000-0000-0000-00000000000a/resourceGroups/rg1` | - ! | **hold** | **hold** | **hold** |
| `az-sa-open` | hold:exposed | `PATCH management/subscriptions/00000000-0000-0000-0000-00000000000a/resourceGroups/rg1` | - ! | **hold** | **hold** | **hold** |
| `az-sa-key-renew` | hold:outage | `POST management/subscriptions/00000000-0000-0000-0000-00000000000a/resourceGroups/rg1` | - ! | **hold** | **hold** | **hold** |
| `az-blob-delete` | hold:gone | `DELETE sa1/c1/report.csv` | **hold** | **hold** | **hold** | **hold** |
| `az-container-delete` | hold:gone | `DELETE sa1/c1` | **hold** | **hold** | **hold** | **hold** |
| `az-kv-secret-delete` | hold:recoverable | `DELETE kv1/secrets/s1` | **hold** | **hold** | **hold** | **hold** |
| `az-kv-secret-delete` | hold:recoverable | `DELETE kv1/secrets/s1` | **hold** | **hold** | **hold** | **hold** |
| `az-kv-secret-purge` | hold:gone | `DELETE kv1/deletedsecrets/s1` | **hold** | **hold** | **hold** | **hold** |
| `az-kv-secret-purge` | hold:gone | `DELETE kv1/deletedsecrets/s1` | **hold** | **hold** | **hold** | **hold** |
| `az-kv-secret-set` | either | `PUT kv1/secrets/s1` | - | - | - | - |
| `az-kv-secret-set` | either | `PUT kv1/secrets/s1` | - | - | - | - |
| `az-kv-delete` | hold:recoverable | `DELETE management/subscriptions/00000000-0000-0000-0000-00000000000a/resourceGroups/rg1` | **hold** | **hold** | **hold** | **hold** |
| `az-kv-policy` | hold:exposed | `PUT management/subscriptions/00000000-0000-0000-0000-00000000000a/resourceGroups/rg1` | - ! | **hold** | **hold** | **hold** |
| `az-sql-db-delete` | hold:gone | `DELETE management/subscriptions/00000000-0000-0000-0000-00000000000a/resourceGroups/rg1` | **hold** | **hold** | **hold** | **hold** |
| `az-sql-fw-open` | hold:exposed | `PUT management/subscriptions/00000000-0000-0000-0000-00000000000a/resourceGroups/rg1` | - ! | **hold** | **hold** | **hold** |
| `az-pg-delete` | hold:gone | `DELETE management/subscriptions/00000000-0000-0000-0000-00000000000a/resourceGroups/rg1` | **hold** | **hold** | **hold** | **hold** |
| `az-nsg-open` | hold:exposed | `PUT management/subscriptions/00000000-0000-0000-0000-00000000000a/resourceGroups/rg1` | **hold** | **hold** | **hold** | **hold** |
| `az-nsg-private` | either | `PUT management/subscriptions/00000000-0000-0000-0000-00000000000a/resourceGroups/rg1` | - | - | - | - |
| `az-aks-delete` | hold:gone | `DELETE management/subscriptions/00000000-0000-0000-0000-00000000000a/resourceGroups/rg1` | **hold** | **hold** | **hold** | **hold** |
| `az-aks-nodepool-delete` | hold:outage | `DELETE management/subscriptions/00000000-0000-0000-0000-00000000000a/resourceGroups/rg1` | **hold** | **hold** | **hold** | **hold** |
| `az-aks-scale-zero` | hold:outage | `PUT management/subscriptions/00000000-0000-0000-0000-00000000000a/resourceGroups/rg1` | **hold** | **hold** | **hold** | **hold** |
| `az-aks-admin-creds` | either | `POST management/subscriptions/00000000-0000-0000-0000-00000000000a/resourceGroups/rg1` | - | - | - | - |
| `az-webapp-delete` | hold:recoverable | `DELETE management/subscriptions/00000000-0000-0000-0000-00000000000a/resourceGroups/rg1` | **hold** | **hold** | **hold** | **hold** |
| `az-webapp-stop` | hold:outage | `POST management/subscriptions/00000000-0000-0000-0000-00000000000a/resourceGroups/rg1` | **hold** | - ! | **hold** | **hold** |
| `az-webapp-restart` | either | `POST management/subscriptions/00000000-0000-0000-0000-00000000000a/resourceGroups/rg1` | - | - | - | - |
| `az-role-owner` | hold:exposed | `PUT management/subscriptions/00000000-0000-0000-0000-00000000000a/providers/Microsof` | **hold** | **hold** | **hold** | **hold** |
| `az-lock-delete` | hold:outage | `DELETE management/subscriptions/00000000-0000-0000-0000-00000000000a/resourceGroups/rg1` | **hold** | **hold** | **hold** | **hold** |
| `az-cosmos-delete` | hold:gone | `DELETE management/subscriptions/00000000-0000-0000-0000-00000000000a/resourceGroups/rg1` | **hold** | **hold** | **hold** | **hold** |
| `az-redis-delete` | hold:gone | `DELETE management/subscriptions/00000000-0000-0000-0000-00000000000a/resourceGroups/rg1` | **hold** | **hold** | **hold** | **hold** |
| `az-acr-delete` | hold:gone | `DELETE management/subscriptions/00000000-0000-0000-0000-00000000000a/resourceGroups/rg1` | **hold** | **hold** | **hold** | **hold** |
| `az-acr-admin` | hold:exposed | `PATCH management/subscriptions/00000000-0000-0000-0000-00000000000a/resourceGroups/rg1` | - ! | **hold** | **hold** | **hold** |
| `az-deploy-complete` | hold:gone | `POST management/subscriptions/00000000-0000-0000-0000-00000000000a/resourcegroups/rg1` | - ! | - ! | - ! | - ! |
| `az-deploy-complete` | hold:gone | `PUT management/subscriptions/00000000-0000-0000-0000-00000000000a/resourcegroups/rg1` | - ! | - ! | - ! | **hold** |
| `az-deploy-incremental` | deploy | `POST management/subscriptions/00000000-0000-0000-0000-00000000000a/resourcegroups/rg1` | - | - | - | - |
| `az-deploy-incremental` | deploy | `PUT management/subscriptions/00000000-0000-0000-0000-00000000000a/resourcegroups/rg1` | - | - | - | **hold** |
| `az-public-ip-delete` | hold:outage | `DELETE management/subscriptions/00000000-0000-0000-0000-00000000000a/resourceGroups/rg1` | **hold** | **hold** | **hold** | **hold** |
| `az-group-list` | allow | (a read only) | | | | |

## gh

| Command | Label | Write | fallback | blind | blind+fb | tuned |
|---|---|---|---|---|---|---|
| `gh-repo-delete` | hold:gone | `DELETE api/repos/acme/app` | **hold** | **hold** | **hold** | **hold** |
| `gh-repo-archive` | hold:outage | `POST api/graphql` | **hold** | **hold** | **hold** | **hold** |
| `gh-repo-public` | hold:exposed | `PATCH api/repos/acme/app` | **hold** | **hold** | **hold** | **hold** |
| `gh-repo-describe` | allow | `PATCH api/repos/acme/app` | - | - | - | - |
| `gh-release-delete` | hold:gone | `DELETE api/repos/acme/app/releases/5` | **hold** | **hold** | **hold** | **hold** |
| `gh-release-delete` | hold:gone | `DELETE api/repos/acme/app/git/refs/tags/v1` | - ! | **hold** | **hold** | **hold** |
| `gh-secret-delete` | hold:gone | `DELETE api/repos/acme/app/actions/secrets/DB_URL` | **hold** | **hold** | **hold** | **hold** |
| `gh-secret-set` | either | `PUT api/repos/acme/app/actions/secrets/FLAG` | - | - | - | - |
| `gh-variable-delete` | hold:gone | `DELETE api/repos/acme/app/actions/variables/FLAG` | **hold** | **hold** | **hold** | **hold** |
| `gh-ref-delete` | hold:recoverable | `DELETE api/repos/acme/app/git/refs/heads/feature` | **hold** | **hold** | **hold** | **hold** |
| `gh-ref-force` | hold:recoverable | `PATCH api/repos/acme/app/git/refs/heads/main` | **hold** | **hold** | **hold** | **hold** |
| `gh-protection-delete` | hold:outage | `DELETE api/repos/acme/app/branches/main/protection` | **hold** | **hold** | **hold** | **hold** |
| `gh-pr-merge` | deploy | `POST api/graphql` | - | - | - | - |
| `gh-pr-close` | allow | `POST api/graphql` | - | - | - | - |
| `gh-issue-delete` | hold:gone | `POST api/graphql` | **hold** | **hold** | **hold** | **hold** |
| `gh-issue-close` | allow | `POST api/graphql` | - | - | - | - |
| `gh-label-delete` | allow | `DELETE api/repos/acme/app/labels/bug` | - | - | - | - |
| `gh-run-delete` | hold:gone | `DELETE api/repos/acme/app/actions/runs/123` | **hold** | **hold** | **hold** | **hold** |
| `gh-cache-delete` | allow | `DELETE api/repos/acme/app/actions/caches/1` | - | - | - | - |
| `gh-workflow-disable` | hold:outage | `PUT api/repos/acme/app/actions/workflows/7/disable` | **hold** | **hold** | **hold** | **hold** |
| `gh-deploy-key-add` | hold:exposed | `POST api/repos/acme/app/keys` | **hold** | **hold** | **hold** | **hold** |
| `gh-deploy-key-delete` | hold:outage | `DELETE api/repos/acme/app/keys/1` | **hold** | **hold** | **hold** | **hold** |
| `gh-collaborator-admin` | hold:exposed | `PUT api/repos/acme/app/collaborators/someone` | **hold** | **hold** | **hold** | **hold** |
| `gh-member-remove` | hold:outage | `DELETE api/orgs/acme/members/someone` | **hold** | **hold** | **hold** | **hold** |
| `gh-ssh-key-add` | hold:exposed | `POST api/user/keys` | **hold** | **hold** | **hold** | **hold** |
| `gh-sync-force` | hold:recoverable | `POST api/repos/acme/app/merge-upstream` | - ! | - ! | - ! | - ! |
| `gh-gql-delete-ref` | hold:recoverable | `POST api/graphql` | **hold** | **hold** | **hold** | **hold** |
| `gh-project-delete` | hold:gone | (gh could not tell from the mock whether acme is a user or an organization; nothing was sent) | | | | |
| `gh-repo-create` | allow | `POST api/graphql` | - | - | - | - |
| `gh-repo-list` | allow | (a read only) | | | | |

## kubectl

| Command | Label | Write | fallback | blind | blind+fb | tuned |
|---|---|---|---|---|---|---|
| `k-ns-delete` | hold:gone | `DELETE kube/api/v1/namespaces/prod` | **hold** | **hold** | **hold** | **hold** |
| `k-deploy-delete` | hold:outage | `DELETE kube/apis/apps/v1/namespaces/prod/deployments/web` | **hold** | **hold** | **hold** | **hold** |
| `k-pod-delete` | either | `DELETE kube/api/v1/namespaces/prod/pods/web-1` | **hold** | - | - | - |
| `k-pvc-delete` | hold:gone | `DELETE kube/api/v1/namespaces/prod/persistentvolumeclaims/data-db-0` | **hold** | **hold** | **hold** | **hold** |
| `k-secret-delete` | hold:gone | `DELETE kube/api/v1/namespaces/prod/secrets/db-creds` | **hold** | **hold** | **hold** | **hold** |
| `k-pods-all-delete` | hold:outage | `DELETE kube/api/v1/namespaces/prod/pods/web-1` | **hold** | - ! | - ! | - ! |
| `k-pods-all-delete` | hold:outage | `DELETE kube/api/v1/namespaces/prod/pods/web-2` | **hold** | - ! | - ! | - ! |
| `k-crd-delete` | hold:gone | `DELETE kube/apis/apiextensions.k8s.io/v1/customresourcedefinitions/widgets.exampl` | **hold** | **hold** | **hold** | **hold** |
| `k-scale-zero` | hold:outage | `PATCH kube/apis/apps/v1/namespaces/prod/deployments/web/scale` | **hold** | **hold** | **hold** | **hold** |
| `k-scale-three` | either | `PATCH kube/apis/apps/v1/namespaces/prod/deployments/web/scale` | - | - | - | - |
| `k-restart` | either | `PATCH kube/apis/apps/v1/namespaces/prod/deployments/web` | - | - | - | - |
| `k-set-image` | deploy | `PATCH kube/apis/apps/v1/namespaces/prod/deployments/web` | - | - | - | - |
| `k-cordon` | hold:outage | `PATCH kube/api/v1/nodes/node1` | - ! | **hold** | **hold** | **hold** |
| `k-drain` | hold:outage | `PATCH kube/api/v1/nodes/node1` | - ! | **hold** | **hold** | **hold** |
| `k-drain` | hold:outage | `POST kube/api/v1/namespaces/prod/pods/web-1/eviction` | - ! | **hold** | **hold** | **hold** |
| `k-uncordon` | allow | (a read only) | | | | |
| `k-taint` | hold:outage | `PATCH kube/api/v1/nodes/node1` | - ! | **hold** | **hold** | **hold** |
| `k-crb-admin` | hold:exposed | `POST kube/apis/rbac.authorization.k8s.io/v1/clusterrolebindings` | **hold** | **hold** | **hold** | **hold** |
| `k-token` | hold:exposed | `POST kube/api/v1/namespaces/prod/serviceaccounts/default/token` | **hold** | **hold** | **hold** | **hold** |
| `k-expose-lb` | hold:exposed | `POST kube/api/v1/namespaces/prod/services` | - ! | **hold** | **hold** | **hold** |
| `k-patch-svc-lb` | hold:exposed | `PATCH kube/api/v1/namespaces/prod/services/db` | - ! | **hold** | **hold** | **hold** |
| `k-apply-zero` | hold:outage | (kubectl validates the manifest against the server's OpenAPI document and gave up on the mock's; nothing was sent) | | | | |
| `k-replace-force` | hold:outage | `DELETE kube/apis/apps/v1/namespaces/prod/deployments/web` | **hold** | **hold** | **hold** | **hold** |
| `k-delete-file` | hold:outage | `DELETE kube/apis/apps/v1/namespaces/prod/deployments/web` | **hold** | **hold** | **hold** | **hold** |
| `k-netpol-delete` | hold:exposed | `DELETE kube/apis/networking.k8s.io/v1/namespaces/prod/networkpolicies/deny-all` | **hold** | **hold** | **hold** | **hold** |
| `k-webhook-delete` | hold:exposed | `DELETE kube/apis/admissionregistration.k8s.io/v1/validatingwebhookconfigurations/` | **hold** | **hold** | **hold** | **hold** |
| `k-csr-approve` | hold:exposed | `PUT kube/apis/certificates.k8s.io/v1/certificatesigningrequests/csr1/approval` | - ! | **hold** | **hold** | **hold** |
| `k-label` | allow | `PATCH kube/api/v1/namespaces/prod/pods/web-1` | - | - | - | - |
| `k-ns-create` | allow | `POST kube/api/v1/namespaces` | - | - | - | - |
| `k-secret-create` | allow | `POST kube/api/v1/namespaces/dev/secrets` | - | - | - | - |
| `k-job-delete` | either | `DELETE kube/apis/batch/v1/namespaces/prod/jobs/migrate` | **hold** | - | - | - |
| `k-get` | allow | (a read only) | | | | |
