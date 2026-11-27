"""Labels for Google Cloud (16 APIs, the versions gcloud 570.0.0 calls): a default by the method's
name, then every method read by hand and the default corrected (OVERRIDE, keyed by method id).
Writes labels/gcloud.txt. Words as in label_aws.py.
"""
import json, os, re

HERE = os.path.dirname(os.path.abspath(__file__))

READS = re.compile(r"^(get|list|test|query|validate|lint|preview|wait|batchGet|runQuery|runAggregationQuery|"
                   r"partitionQuery|listen|listCollectionIds|readQueryResults|tail|check|verify|preCheck|calendarMode|"
                   r"getHealth|getVersion|generateRandomBytes|macVerify|aggregated|beginTransaction|executePipeline)")
DEFAULT = [
    (r"^(setLabels|bulkSetLabels|setTags|setResourceLabels|resourceLabels)$", "allow", "labels"),
    (r"^(delete|batchDelete|deleteRevision)", "hold:outage", ""),
    (r"^destroy$", "hold:gone", ""),
    (r"^(stop|suspend|disable|pause|withdraw|detach|remove|abandon)", "hold:outage", ""),
    (r"^(reset)", "hold:outage", ""),
    (r"^(restart|failover|switchover|rollback|demote|promote)", "either", "a short interruption"),
    (r"^setIamPolicy$", "cond", ""),
    (r"^(patch|update|set|replace|modify|move|relocate)", "either", "changes a setting"),
    (r".", "allow", ""),
]

G = "hold:gone"
O = "hold:outage"
X = "hold:exposed"
R = "hold:recoverable"
E = "either"
A = "allow"
D = "deploy"
C = "cond"
M = "hold:money"

OVERRIDE = {
    # compute: data that goes with the call
    "compute.instances.delete": (G, "boot and data disks with auto-delete go with it"),
    "compute.disks.delete": (G, ""), "compute.regionDisks.delete": (G, ""),
    "compute.snapshots.delete": (G, ""), "compute.regionSnapshots.delete": (G, ""),
    "compute.images.delete": (G, ""), "compute.machineImages.delete": (G, ""),
    "compute.instantSnapshots.delete": (G, ""), "compute.regionInstantSnapshots.delete": (G, ""),
    "compute.instantSnapshotGroups.delete": (G, ""), "compute.regionInstantSnapshotGroups.delete": (G, ""),
    "compute.instanceGroupManagers.deleteInstances": (G, ""), "compute.regionInstanceGroupManagers.deleteInstances": (G, ""),
    "compute.instanceGroupManagers.delete": (G, "its instances go"), "compute.regionInstanceGroupManagers.delete": (G, "its instances go"),
    "compute.nodeGroups.deleteNodes": (O, ""),
    "compute.instances.reset": (O, "a hard reset"), "compute.instances.sendDiagnosticInterrupt": (O, ""),
    "compute.instances.simulateMaintenanceEvent": (E, ""), "compute.nodeGroups.simulateMaintenanceEvent": (E, ""),
    "compute.instances.setDeletionProtection": (C, ""), "compute.instances.setDiskAutoDelete": (E, ""),
    "compute.instances.addAccessConfig": (X, "gives the instance an external IP"),
    "compute.instances.updateAccessConfig": (E, ""), "compute.instances.deleteAccessConfig": (O, "its external IP goes"),
    "compute.instances.setMetadata": (C, ""), "compute.projects.setCommonInstanceMetadata": (C, ""),
    "compute.instances.setServiceAccount": (X, "changes what the instance may do"),
    "compute.instances.setMachineType": (E, ""), "compute.instances.setMachineResources": (E, ""),
    "compute.instances.setName": (E, ""), "compute.instances.setScheduling": (E, ""),
    "compute.instances.setMinCpuPlatform": (A, ""), "compute.instances.setShieldedInstanceIntegrityPolicy": (A, ""),
    "compute.instances.updateShieldedInstanceConfig": (E, ""), "compute.instances.performMaintenance": (E, ""),
    "compute.instances.reportHostAsFaulty": (E, ""), "compute.instances.resume": (A, ""),
    "compute.instances.updateDisplayDevice": (A, ""), "compute.instances.updateNetworkInterface": (E, ""),
    "compute.instances.deleteNetworkInterface": (O, ""), "compute.instances.detachDisk": (O, ""),
    "compute.instanceGroupManagers.resize": (C, ""), "compute.regionInstanceGroupManagers.resize": (C, ""),
    "compute.instanceGroupManagers.recreateInstances": (O, "recreated from the template; local data goes"),
    "compute.regionInstanceGroupManagers.recreateInstances": (O, ""),
    "compute.instanceGroupManagers.applyUpdatesToInstances": (D, ""), "compute.regionInstanceGroupManagers.applyUpdatesToInstances": (D, ""),
    "compute.instanceGroupManagers.setInstanceTemplate": (D, ""), "compute.regionInstanceGroupManagers.setInstanceTemplate": (D, ""),
    "compute.instanceGroupManagers.abandonInstances": (E, ""), "compute.regionInstanceGroupManagers.abandonInstances": (E, ""),
    "compute.instanceGroupManagers.deletePerInstanceConfigs": (E, ""), "compute.regionInstanceGroupManagers.deletePerInstanceConfigs": (E, ""),
    "compute.firewalls.insert": (C, ""), "compute.firewalls.patch": (C, ""), "compute.firewalls.update": (C, ""),
    "compute.firewalls.delete": (E, "a deny rule removed opens; an allow rule removed closes"),
    "compute.firewallPolicies.addRule": (E, ""), "compute.firewallPolicies.removeRule": (E, ""),
    "compute.networkFirewallPolicies.addRule": (E, ""), "compute.networkFirewallPolicies.removeRule": (E, ""),
    "compute.regionNetworkFirewallPolicies.addRule": (E, ""), "compute.regionNetworkFirewallPolicies.removeRule": (E, ""),
    "compute.firewallPolicies.removeAssociation": (X, "its rules stop applying"),
    "compute.networkFirewallPolicies.removeAssociation": (X, ""), "compute.regionNetworkFirewallPolicies.removeAssociation": (X, ""),
    "compute.organizationSecurityPolicies.removeAssociation": (X, ""),
    "compute.securityPolicies.removeRule": (E, ""), "compute.regionSecurityPolicies.removeRule": (E, ""),
    "compute.organizationSecurityPolicies.removeRule": (E, ""),
    "compute.backendServices.setSecurityPolicy": (E, ""), "compute.regionBackendServices.setSecurityPolicy": (E, ""),
    "compute.backendServices.setEdgeSecurityPolicy": (E, ""), "compute.backendBuckets.setEdgeSecurityPolicy": (E, ""),
    "compute.instances.setSecurityPolicy": (E, ""), "compute.targetInstances.setSecurityPolicy": (E, ""),
    "compute.targetPools.setSecurityPolicy": (E, ""),
    "compute.backendBuckets.addSignedUrlKey": (X, ""), "compute.backendServices.addSignedUrlKey": (X, ""),
    "compute.backendBuckets.deleteSignedUrlKey": (O, ""), "compute.backendServices.deleteSignedUrlKey": (O, ""),
    "compute.networks.addPeering": (X, ""), "compute.networks.removePeering": (O, ""),
    "compute.networks.requestRemovePeering": (O, ""), "compute.networks.cancelRequestRemovePeering": (A, ""),
    "compute.networks.switchToCustomMode": (E, ""),
    "compute.subnetworks.setPrivateIpGoogleAccess": (A, ""), "compute.subnetworks.expandIpCidrRange": (A, ""),
    "compute.projects.enableXpnHost": (X, ""), "compute.projects.enableXpnResource": (X, ""),
    "compute.projects.disableXpnHost": (O, ""), "compute.projects.disableXpnResource": (O, ""),
    "compute.projects.setUsageExportBucket": (E, ""), "compute.projects.moveDisk": (E, ""),
    "compute.projects.moveInstance": (E, ""),
    "compute.projects.setCloudArmorTier": (M, ""), "compute.projects.setDefaultNetworkTier": (E, ""),
    "compute.regionCommitments.insert": (M, "a one- or three-year commitment"), "compute.regionCommitments.update": (M, ""),
    "compute.reservations.insert": (M, ""), "compute.futureReservations.insert": (M, ""),
    "compute.reservations.resize": (E, ""), "compute.reservations.delete": (O, ""),
    "compute.futureReservations.cancel": (A, ""), "compute.futureReservations.delete": (A, ""),
    "compute.globalOperations.delete": (A, ""), "compute.regionOperations.delete": (A, ""),
    "compute.zoneOperations.delete": (A, ""), "compute.globalOrganizationOperations.delete": (A, ""),
    "compute.instanceGroupManagerResizeRequests.delete": (A, ""), "compute.regionInstanceGroupManagerResizeRequests.delete": (A, ""),
    "compute.instanceTemplates.delete": (A, "running instances stay"), "compute.regionInstanceTemplates.delete": (A, ""),
    "compute.nodeTemplates.delete": (A, ""), "compute.resourcePolicies.delete": (E, ""),
    "compute.images.deprecate": (E, ""), "compute.disks.resize": (A, "only larger"), "compute.regionDisks.resize": (A, ""),
    "compute.disks.createSnapshot": (A, ""), "compute.regionDisks.createSnapshot": (A, ""),
    "compute.disks.removeResourcePolicies": (E, "may stop scheduled snapshots"),
    "compute.regionDisks.removeResourcePolicies": (E, ""), "compute.instances.removeResourcePolicies": (E, ""),
    "compute.disks.stopAsyncReplication": (O, ""), "compute.regionDisks.stopAsyncReplication": (O, ""),
    "compute.disks.stopGroupAsyncReplication": (O, ""), "compute.regionDisks.stopGroupAsyncReplication": (O, ""),
    "compute.disks.updateKmsKey": (E, ""), "compute.regionDisks.updateKmsKey": (E, ""),
    "compute.snapshots.updateKmsKey": (E, ""), "compute.regionSnapshots.updateKmsKey": (E, ""),
    "compute.globalNetworkEndpointGroups.listNetworkEndpoints": (A, ""), "compute.networkEndpointGroups.listNetworkEndpoints": (A, ""),
    "compute.regionNetworkEndpointGroups.listNetworkEndpoints": (A, ""),
    "compute.instanceGroupManagers.listManagedInstances": (A, ""), "compute.instanceGroupManagers.listPerInstanceConfigs": (A, ""),
    "compute.regionInstanceGroupManagers.listManagedInstances": (A, ""), "compute.regionInstanceGroupManagers.listPerInstanceConfigs": (A, ""),
    "compute.instanceGroups.listInstances": (A, ""), "compute.regionInstanceGroups.listInstances": (A, ""),
    "compute.nodeGroups.listNodes": (A, ""), "compute.projects.listXpnHosts": (A, ""),
    "compute.urlMaps.invalidateCache": (A, ""), "compute.routers.preview": (A, ""),
    "compute.publicAdvertisedPrefixes.withdraw": (O, ""), "compute.publicDelegatedPrefixes.withdraw": (O, ""),
    "compute.rollouts.pause": (E, ""), "compute.rollouts.cancel": (E, ""), "compute.rollouts.advance": (D, ""),
    "compute.snapshotSettings.patch": (E, ""), "compute.regionSnapshotSettings.patch": (E, ""),
    "compute.instanceSettings.patch": (E, ""), "compute.previewFeatures.update": (E, ""),
    "compute.addresses.move": (E, ""), "compute.globalAddresses.move": (E, ""),
    # storage
    "storage.buckets.delete": (O, "only an empty bucket"), "storage.objects.delete": (G, "unless soft delete or versioning keeps it"),
    "storage.folders.deleteRecursive": (G, ""), "storage.folders.delete": (A, "only empty"),
    "storage.managedFolders.delete": (A, "only empty"), "storage.notifications.delete": (O, ""),
    "storage.objects.insert": (E, "overwrites the name"), "storage.objects.copy": (E, ""), "storage.objects.rewrite": (E, ""),
    "storage.objects.compose": (E, ""), "storage.objects.move": (E, ""), "storage.objects.restore": (A, ""),
    "storage.objects.bulkRestore": (A, ""), "storage.buckets.restore": (A, ""), "storage.objects.watchAll": (A, ""),
    "storage.channels.stop": (A, ""), "storage.buckets.patch": (C, ""), "storage.buckets.update": (C, ""),
    "storage.objects.patch": (E, ""), "storage.objects.update": (E, ""),
    "storage.buckets.lockRetentionPolicy": (M, "cannot be undone; the bucket cannot be deleted until objects age"),
    "storage.bucketAccessControls.insert": (C, ""), "storage.bucketAccessControls.patch": (C, ""),
    "storage.bucketAccessControls.update": (C, ""), "storage.defaultObjectAccessControls.insert": (C, ""),
    "storage.defaultObjectAccessControls.patch": (C, ""), "storage.defaultObjectAccessControls.update": (C, ""),
    "storage.objectAccessControls.insert": (C, ""), "storage.objectAccessControls.patch": (C, ""),
    "storage.objectAccessControls.update": (C, ""), "storage.bucketAccessControls.delete": (E, ""),
    "storage.defaultObjectAccessControls.delete": (E, ""), "storage.objectAccessControls.delete": (E, ""),
    "storage.projects.hmacKeys.create": (X, ""), "storage.projects.hmacKeys.update": (C, ""),
    "storage.projects.hmacKeys.delete": (A, "only an inactive key"),
    "storage.anywhereCaches.disable": (A, ""), "storage.anywhereCaches.pause": (A, ""),
    "storage.buckets.operations.cancel": (A, ""), "storage.buckets.operations.advanceRelocateBucket": (E, ""),
    # Cloud SQL
    "sql.instances.delete": (G, "its automated backups go too, unless kept"), "sql.databases.delete": (G, ""),
    "sql.backupRuns.delete": (G, ""), "sql.backups.deleteBackup": (G, ""),
    "sql.instances.restoreBackup": (G, "overwrites the instance's data"),
    "sql.instances.patch": (C, ""), "sql.instances.update": (C, ""), "sql.instances.executeSql": (C, ""),
    "sql.instances.import": (E, "overwrites"), "sql.instances.export": (E, "to a bucket"),
    "sql.instances.truncateLog": (E, ""), "sql.instances.clone": (A, ""), "sql.instances.pointInTimeRestore": (A, "a new instance"),
    "sql.instances.resetSslConfig": (O, "clients' certificates stop"), "sql.instances.stopReplica": (O, ""),
    "sql.instances.removeReplicationSource": (O, ""), "sql.instances.demoteMaster": (O, ""), "sql.instances.demote": (O, ""),
    "sql.instances.promoteReplica": (E, ""), "sql.instances.rotateServerCa": (E, ""),
    "sql.instances.RotateServerCertificate": (E, ""), "sql.instances.RotateEntraIdCertificate": (E, ""),
    "sql.users.insert": (X, ""), "sql.users.update": (X, "may set a password"), "sql.users.delete": (O, ""),
    "sql.sslCerts.insert": (X, ""), "sql.sslCerts.createEphemeral": (E, ""), "sql.connect.generateEphemeral": (E, "a short-lived certificate to connect"),
    "sql.sslCerts.delete": (O, ""), "sql.blueGreenDeployments.delete": (E, ""),
    "sql.blueGreenDeployments.switchover": (E, ""), "sql.projects.instances.performDiskShrink": (E, ""),
    "sql.projects.instances.resetReplicaSize": (E, ""), "sql.instances.reencrypt": (E, ""),
    "sql.instances.acquireSsrsLease": (A, ""), "sql.instances.releaseSsrsLease": (A, ""),
    "sql.operations.cancel": (A, ""), "sql.databases.patch": (E, ""), "sql.databases.update": (E, ""),
    # GKE
    "container.projects.locations.clusters.delete": (G, "workloads and their volumes go"), "container.projects.zones.clusters.delete": (G, ""),
    "container.projects.locations.clusters.update": (C, ""), "container.projects.zones.clusters.update": (C, ""),
    "container.projects.locations.clusters.setMasterAuth": (X, ""), "container.projects.zones.clusters.setMasterAuth": (X, ""),
    "container.projects.locations.clusters.setLegacyAbac": (C, ""), "container.projects.zones.clusters.legacyAbac": (C, ""),
    "container.projects.locations.clusters.setNetworkPolicy": (E, ""), "container.projects.zones.clusters.setNetworkPolicy": (E, ""),
    "container.projects.locations.clusters.nodePools.setSize": (C, ""), "container.projects.zones.clusters.nodePools.setSize": (C, ""),
    "container.projects.locations.clusters.nodePools.setAutoscaling": (E, ""), "container.projects.zones.clusters.nodePools.autoscaling": (E, ""),
    "container.projects.locations.clusters.completeIpRotation": (O, "clients with the old address stop"),
    "container.projects.zones.clusters.completeIpRotation": (O, ""),
    "container.projects.zones.clusters.addons": (E, ""), "container.projects.zones.clusters.locations": (E, ""),
    "container.projects.zones.clusters.logging": (E, ""), "container.projects.zones.clusters.master": (E, ""),
    "container.projects.zones.clusters.monitoring": (E, ""), "container.projects.zones.clusters.resourceLabels": (A, ""),
    "container.projects.locations.operations.cancel": (A, ""), "container.projects.zones.operations.cancel": (A, ""),
    "container.projects.locations.acceleratorNetworkProfiles.delete": (A, ""),
    # Cloud Run
    "run.namespaces.services.delete": (O, ""), "run.projects.locations.services.delete": (O, ""),
    "run.namespaces.services.replaceService": (D, ""), "run.projects.locations.services.replaceService": (D, ""),
    "run.namespaces.services.create": (D, ""), "run.projects.locations.services.create": (D, ""),
    "run.namespaces.jobs.replaceJob": (D, ""), "run.namespaces.workerpools.replaceWorkerPool": (D, ""),
    "run.namespaces.instances.replaceInstance": (D, ""), "run.namespaces.jobs.run": (E, "runs the job"),
    "run.namespaces.executions.delete": (A, ""), "run.namespaces.executions.cancel": (A, ""),
    "run.namespaces.revisions.delete": (E, "an old revision, or one taking traffic"), "run.projects.locations.revisions.delete": (E, ""),
    "run.projects.locations.operations.delete": (A, ""),
    # IAM
    "iam.projects.serviceAccounts.keys.create": (X, ""), "iam.projects.serviceAccounts.keys.upload": (X, ""),
    "iam.projects.serviceAccounts.signBlob": (X, "signs as the service account"), "iam.projects.serviceAccounts.signJwt": (X, "signs as the service account"),
    "iam.projects.serviceAccounts.delete": (O, "30 days to undelete"), "iam.projects.serviceAccounts.keys.delete": (O, ""),
    "iam.projects.serviceAccounts.getIamPolicy": (A, ""), "iam.locations.workforcePools.getIamPolicy": (A, ""),
    "iam.projects.locations.workloadIdentityPools.getIamPolicy": (A, ""),
    "iam.projects.locations.workloadIdentityPools.namespaces.getIamPolicy": (A, ""),
    "iam.projects.locations.workloadIdentityPools.namespaces.managedIdentities.getIamPolicy": (A, ""),
    "iam.projects.locations.workloadIdentityPools.create": (X, "lets outside identities in"),
    "iam.projects.locations.workloadIdentityPools.providers.create": (X, ""),
    "iam.projects.locations.workloadIdentityPools.providers.patch": (X, ""),
    "iam.projects.locations.workloadIdentityPools.providers.keys.create": (X, ""),
    "iam.locations.workforcePools.create": (X, ""), "iam.locations.workforcePools.providers.create": (X, ""),
    "iam.locations.workforcePools.providers.patch": (X, ""), "iam.locations.workforcePools.providers.keys.create": (X, ""),
    "iam.locations.workforcePools.providers.scimTenants.tokens.create": (X, ""),
    "iam.projects.locations.oauthClients.credentials.create": (X, ""), "iam.projects.locations.oauthClients.create": (E, ""),
    "iam.projects.locations.workloadIdentityPools.addAttestationRule": (X, ""),
    "iam.projects.locations.workloadIdentityPools.namespaces.addAttestationRule": (X, ""),
    "iam.projects.locations.workloadIdentityPools.namespaces.managedIdentities.addAttestationRule": (X, ""),
    "iam.projects.roles.create": (A, ""), "iam.organizations.roles.create": (A, ""),
    "iam.projects.roles.patch": (E, ""), "iam.organizations.roles.patch": (E, ""),
    "iam.locations.workforcePools.subjects.revokeSessions": (O, ""),
    "iam.iamPolicies.lintPolicy": (A, ""), "iam.iamPolicies.queryAuditableServices": (A, ""),
    "iam.permissions.queryTestablePermissions": (A, ""), "iam.roles.queryGrantableRoles": (A, ""),
    # Resource Manager
    "cloudresourcemanager.projects.delete": (R, "30 days to undelete, then gone with everything in it"),
    "cloudresourcemanager.folders.delete": (A, "only an empty folder"),
    "cloudresourcemanager.projects.getIamPolicy": (A, ""), "cloudresourcemanager.folders.getIamPolicy": (A, ""),
    "cloudresourcemanager.organizations.getIamPolicy": (A, ""), "cloudresourcemanager.tagKeys.getIamPolicy": (A, ""),
    "cloudresourcemanager.tagValues.getIamPolicy": (A, ""),
    "cloudresourcemanager.liens.delete": (O, "a protection against deleting the project goes"),
    "cloudresourcemanager.projects.move": (X, ""), "cloudresourcemanager.folders.move": (X, ""),
    "cloudresourcemanager.tagKeys.delete": (A, ""), "cloudresourcemanager.tagValues.delete": (A, ""),
    "cloudresourcemanager.tagBindings.delete": (E, "tags may grant access"), "cloudresourcemanager.tagBindings.create": (E, ""),
    "cloudresourcemanager.tagValues.tagHolds.delete": (A, ""),
    "cloudresourcemanager.projects.settings.clear": (E, ""), "cloudresourcemanager.folders.settings.clear": (E, ""),
    "cloudresourcemanager.organizations.settings.clear": (E, ""),
    # Secret Manager
    "secretmanager.projects.secrets.delete": (G, "every version goes"), "secretmanager.projects.locations.secrets.delete": (G, ""),
    "secretmanager.projects.secrets.versions.destroy": (G, ""), "secretmanager.projects.locations.secrets.versions.destroy": (G, ""),
    "secretmanager.projects.secrets.addVersion": (E, "the new value becomes latest"),
    "secretmanager.projects.locations.secrets.addVersion": (E, ""),
    # Pub/Sub
    "pubsub.projects.subscriptions.delete": (G, "its unacknowledged messages go"), "pubsub.projects.snapshots.delete": (G, ""),
    "pubsub.projects.subscriptions.seek": (E, "seeking forward acknowledges messages"),
    "pubsub.projects.subscriptions.modifyPushConfig": (E, "may send messages to an endpoint"),
    "pubsub.projects.subscriptions.acknowledge": (A, ""), "pubsub.projects.subscriptions.modifyAckDeadline": (A, ""),
    "pubsub.projects.subscriptions.pull": (A, ""), "pubsub.projects.topics.publish": (A, ""),
    "pubsub.projects.schemas.deleteRevision": (A, ""), "pubsub.projects.schemas.commit": (E, ""),
    "pubsub.projects.subscriptions.create": (E, "a new subscriber receives every message"),
    "pubsub.projects.testMessageTransforms": (A, ""), "pubsub.projects.validateMessageTransform": (A, ""),
    "pubsub.projects.schemas.validateMessage": (A, ""), "pubsub.projects.schemas.validate": (A, ""),
    # BigQuery
    "bigquery.datasets.delete": (R, "undelete within the time-travel window"), "bigquery.tables.delete": (G, ""),
    "bigquery.models.delete": (G, ""), "bigquery.jobs.insert": (C, ""), "bigquery.jobs.query": (C, ""),
    "bigquery.jobs.delete": (A, ""), "bigquery.jobs.cancel": (A, ""), "bigquery.tabledata.insertAll": (A, ""),
    "bigquery.datasets.patch": (C, ""), "bigquery.datasets.update": (C, ""),
    "bigquery.rowAccessPolicies.delete": (X, "its rows become visible to others"),
    "bigquery.rowAccessPolicies.batchDelete": (X, ""), "bigquery.rowAccessPolicies.getIamPolicy": (A, ""),
    "bigquery.routines.getIamPolicy": (A, ""), "bigquery.tables.getIamPolicy": (A, ""),
    "bigquery.tables.patch": (E, ""), "bigquery.tables.update": (E, "may drop columns or set an expiry"),
    # Cloud Functions
    "cloudfunctions.projects.locations.functions.create": (D, ""), "cloudfunctions.projects.locations.functions.patch": (D, ""),
    "cloudfunctions.projects.locations.functions.generateUploadUrl": (A, ""),
    "cloudfunctions.projects.locations.functions.generateDownloadUrl": (A, ""),
    "cloudfunctions.projects.locations.functions.abortFunctionUpgrade": (A, ""),
    "cloudfunctions.projects.locations.functions.detachFunction": (E, ""),
    # DNS
    "dns.changes.create": (C, ""), "dns.managedZones.getIamPolicy": (A, ""),
    "dns.policies.delete": (E, ""), "dns.responsePolicies.delete": (E, ""), "dns.responsePolicyRules.delete": (E, ""),
    # KMS
    "cloudkms.projects.locations.keyRings.cryptoKeys.cryptoKeyVersions.destroy": (R, "scheduled; 30 days by default, then what it encrypted is lost"),
    "cloudkms.projects.locations.keyRings.cryptoKeys.cryptoKeyVersions.delete": (A, "only a destroyed or failed version"),
    "cloudkms.projects.locations.keyRings.cryptoKeys.delete": (A, "only with every version destroyed"),
    "cloudkms.projects.locations.keyRings.cryptoKeys.cryptoKeyVersions.patch": (C, ""),
    "cloudkms.projects.locations.keyRings.cryptoKeys.cryptoKeyVersions.restore": (A, ""),
    "cloudkms.projects.locations.singleTenantHsmInstances.proposals.delete": (A, ""),
    "cloudkms.projects.locations.singleTenantHsmInstances.proposals.execute": (E, ""),
    "cloudkms.projects.locations.singleTenantHsmInstances.proposals.approve": (E, ""),
    "cloudkms.projects.locations.keyRings.cryptoKeys.cryptoKeyVersions.import": (E, ""),
    # Artifact Registry
    "artifactregistry.projects.locations.repositories.delete": (G, ""), "artifactregistry.projects.locations.repositories.packages.delete": (G, ""),
    "artifactregistry.projects.locations.repositories.packages.versions.delete": (G, ""),
    "artifactregistry.projects.locations.repositories.packages.versions.batchDelete": (G, ""),
    "artifactregistry.projects.locations.repositories.files.delete": (G, ""),
    "artifactregistry.projects.locations.repositories.attachments.delete": (A, ""),
    "artifactregistry.projects.locations.repositories.packages.tags.delete": (E, "deploys that name the tag fail"),
    "artifactregistry.projects.locations.repositories.rules.delete": (E, ""),
    "artifactregistry.projects.locations.repositories.removePrewarmedArtifact": (A, ""),
    "artifactregistry.projects.locations.repositories.exportArtifact": (E, ""),
    "artifactregistry.projects.locations.repositories.reindex": (A, ""),
    "artifactregistry.projects.locations.repositories.packages.tags.patch": (D, "moves a tag"),
    "artifactregistry.projects.locations.repositories.packages.tags.create": (D, ""),
    "artifactregistry.projects.locations.operations.cancel": (A, ""),
    # Logging
    "logging.logs.delete": (G, ""), "logging.projects.logs.delete": (G, ""), "logging.folders.logs.delete": (G, ""),
    "logging.organizations.logs.delete": (G, ""), "logging.billingAccounts.logs.delete": (G, ""),
    "logging.entries.redact": (G, ""), "logging.entries.write": (A, ""), "logging.entries.copy": (A, ""),
    "logging.entries.list": (A, ""), "logging.entries.tail": (A, ""),
    "logging.data.query": (A, ""), "logging.data.queryLocal": (A, ""), "logging.data.querySync": (A, ""),
    "logging.data.readQueryResults": (A, ""), "logging.query.cancel": (A, ""),
    "logging.projects.metrics.delete": (O, "alerts on it stop"),
}
for scope in ("billingAccounts.", "folders.", "organizations.", "projects.", ""):
    pre = "logging." + scope
    OVERRIDE.update({
        pre + "locations.buckets.delete": (R, "7 days to undelete"),
        pre + "sinks.create": (X, "routes logs to a destination"), pre + "sinks.update": (X, ""), pre + "sinks.patch": (X, ""),
        pre + "exclusions.create": (E, "stops keeping some logs"), pre + "exclusions.delete": (A, ""),
        pre + "locations.buckets.links.delete": (A, ""), pre + "locations.buckets.views.delete": (A, ""),
        pre + "locations.savedQueries.delete": (A, ""), pre + "locations.logScopes.delete": (A, ""),
        pre + "locations.operations.cancel": (A, ""), pre + "locations.buckets.patch": (E, "a shorter retention deletes older logs"),
        pre + "locations.buckets.updateAsync": (E, ""),
    })
OVERRIDE.update({
    # Firestore
    "firestore.projects.databases.delete": (G, "unless delete protection is on"),
    "firestore.projects.databases.documents.delete": (G, ""), "firestore.projects.databases.bulkDeleteDocuments": (G, ""),
    "firestore.projects.locations.backups.delete": (G, ""), "firestore.projects.databases.backupSchedules.delete": (E, ""),
    "firestore.projects.databases.documents.batchWrite": (C, ""), "firestore.projects.databases.documents.commit": (C, ""),
    "firestore.projects.databases.documents.write": (E, "a stream"), "firestore.projects.databases.documents.patch": (E, ""),
    "firestore.projects.databases.documents.rollback": (A, ""), "firestore.projects.databases.importDocuments": (E, "overwrites"),
    "firestore.projects.databases.exportDocuments": (E, "to a bucket"), "firestore.projects.databases.patch": (C, ""),
    "firestore.projects.databases.userCreds.create": (X, ""), "firestore.projects.databases.userCreds.resetPassword": (X, ""),
    "firestore.projects.databases.restore": (A, "a new database"), "firestore.projects.databases.clone": (A, ""),
    "firestore.projects.databases.operations.delete": (A, ""), "firestore.projects.databases.operations.cancel": (A, ""),
})


def label(op):
    mid = op["name"]
    o = OVERRIDE.get(mid)
    if o:
        return o
    m = mid.split(".")[-1]
    if READS.match(m):
        return A, "reads"
    for rx, lab, why in DEFAULT:
        if re.match(rx, m):
            return lab, why
    return A, ""


def main():
    ops = json.load(open(os.path.join(HERE, "ops", "gcloud.json")))
    known = {o["name"] for o in ops}
    missing = [k for k in OVERRIDE if k not in known]
    assert not [m for m in missing if not m.startswith("logging.")], missing[:20]
    with open(os.path.join(HERE, "labels", "gcloud.txt"), "w") as f:
        f.write("# Google Cloud (16 APIs, the versions gcloud 570.0.0 calls), labelled by hand on 2026-11-25.\n"
                "# Key: method id. A default by the method's name (label_gcloud.py), every method read and corrected.\n")
        for op in ops:
            lab, why = label(op)
            f.write("%s %s %s\n" % (op["name"], lab, why))
    print("gcloud", len(ops))


if __name__ == "__main__":
    main()
