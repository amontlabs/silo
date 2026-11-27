"""Labels for Azure Resource Manager (14 resource providers, the operations az 2.93.0 and its SDKs
call): ARM is regular, so the label comes from the resource type the path ends in and, for a POST,
the action. Every shape was read by hand (905 distinct shapes) and the tables below are that reading.
Writes labels/azure.txt. Words as in label_aws.py.
"""
import json, os, re

HERE = os.path.dirname(os.path.abspath(__file__))
G, O, X, R, E, A, D, C, M = ("hold:gone", "hold:outage", "hold:exposed", "hold:recoverable", "either", "allow",
                             "deploy", "cond", "hold:money")

# DELETE, by the resource type the path ends in (lower case)
DELETE = {
    "resourcegroups": (G, "every resource in the group goes"),
    "disks": (G, ""), "snapshots": (G, ""), "images": (G, ""), "versions": (G, "an image or archive version"),
    "virtualmachines": (O, "its disks stay unless their delete option says otherwise"),
    "virtualmachinescalesets": (G, "its instances and their OS disks go"),
    "storageaccounts": (G, "every blob, file, queue and table in it"), "containers": (G, "its blobs go"),
    "shares": (G, ""), "queues": (G, ""), "tables": (G, ""),
    "servers": (G, "with its databases"), "databases": (G, ""), "flexibleservers": (G, ""),
    "databaseaccounts": (G, ""), "sqldatabases": (G, ""), "mongodbdatabases": (G, ""), "collections": (G, ""),
    "cassandrakeyspaces": (G, ""), "gremlindatabases": (G, ""), "graphs": (G, ""), "keyspaces": (G, ""),
    "vaults": (R, "soft delete keeps it 7 to 90 days"), "registries": (G, "its images go"),
    "managedclusters": (G, "workloads and their volumes go"), "agentpools": (O, ""),
    "sites": (R, "30 days to restore a deleted app"), "slots": (O, ""), "serverfarms": (O, ""),
    "redis": (G, "the cache's data"), "dnszones": (O, ""), "recordsets": (O, ""),
    "a": (O, ""), "aaaa": (O, ""), "cname": (O, ""), "mx": (O, ""), "txt": (O, ""), "ns": (O, ""), "srv": (O, ""),
    "caa": (O, ""), "ptr": (O, ""),
    "publicipaddresses": (O, ""), "networksecuritygroups": (O, ""), "virtualnetworks": (O, ""), "subnets": (O, ""),
    "networkinterfaces": (O, ""), "loadbalancers": (O, ""), "applicationgateways": (O, ""), "privateendpoints": (O, ""),
    "securityrules": (E, "a deny rule removed opens; an allow rule removed closes"),
    "firewallrules": (E, "removing an allow rule closes"),
    "roleassignments": (O, ""), "roledefinitions": (O, ""), "locks": (O, "a protection against deletion goes"),
    "policyassignments": (X, "a guardrail goes"), "userassignedidentities": (O, ""),
    "federatedidentitycredentials": (O, ""), "privateendpointconnections": (O, ""),
    "replications": (O, ""), "webhooks": (O, ""), "tokens": (O, ""), "scopemaps": (O, ""),
    "credentialsets": (O, ""), "connectedregistries": (O, ""), "cacherules": (A, ""), "agentpools_acr": (O, ""),
    "pipelineruns": (A, ""), "taskruns": (A, ""), "exportpipelines": (A, ""), "importpipelines": (A, ""),
    "archives": (G, ""), "keys": (O, "callers with the key stop"),
    "backuplongtermretentionpolicies": (G, ""), "longtermretentionbackups": (G, ""),
    "deployments": (A, "the deployment's record, not its resources"),
    "extensions": (E, ""), "runcommands": (A, ""), "linkedservers": (O, ""), "patchschedules": (A, ""),
    "accesspolicies": (O, ""), "accesspolicyassignments": (O, ""), "elasticpools": (O, "only empty"),
    "availabilitysets": (A, "only empty"), "galleries": (A, "only empty"), "hybridconnectionnamespaces": (O, ""),
    "hostnamebindings": (O, ""), "domainownershipidentifiers": (A, ""), "premieraddons": (A, ""),
    "functions": (O, ""), "certificates": (O, ""), "sourcecontrols": (A, ""), "virtualnetworkconnections": (O, ""),
    "hybridconnection": (O, ""), "relayserviceconnection": (O, ""), "networkconfig": (O, ""),
    "backups": (G, ""), "privateaccess": (E, ""), "sitecontainers": (O, ""), "workflows": (O, ""),
    "continuouswebjobs": (O, ""), "triggeredwebjobs": (O, ""), "instances": (O, ""), "processes": (O, "kills a process"),
    "deletedvaults": (G, "purges"), "auditingsettings": (E, ""), "default": (O, ""), "tags": (A, ""),
    "administrators": (O, ""), "configurations": (E, ""), "migrations": (A, ""), "sqlroleassignments": (O, ""),
    "sqlroledefinitions": (O, ""), "mongoroledefinitions": (O, ""), "mongouserdefinitions": (O, ""),
    "throughputpools": (O, ""), "throughputpoolaccounts": (O, ""), "services": (O, ""), "storedprocedures": (O, ""),
    "triggers": (O, ""), "userdefinedfunctions": (O, ""), "clientencryptionkeys": (O, ""), "tablespaces": (O, ""),
    "views": (A, ""), "graphresourcegetresults": (A, ""), "restorepoints": (G, ""), "sensitivitylabels": (A, ""),
    "vulnerabilityassessments": (E, ""), "baselines": (A, ""), "azureadonlyauthentications": (X, "SQL logins work again"),
    "virtualnetworkpeerings": (O, ""), "outboundfirewallrules": (E, ""), "ipv6firewallrules": (E, ""),
    "localusers": (O, ""), "managementpolicies": (A, "keeps data longer"), "workloadgroups": (A, ""),
    "workloadclassifiers": (A, ""), "syncgroups": (E, ""), "syncmembers": (E, ""), "jobagents": (O, ""),
    "jobs": (O, ""), "steps": (O, ""), "targetgroups": (A, ""), "credentials": (O, ""), "failovergroups": (O, ""),
    "dnsaliases": (O, ""), "replicationlinks": (O, ""), "deploymentstacks": (C, ""), "deploymentscripts": (A, ""), "chaosfaults": (A, ""), "networksecurityperimeterconfigurations": (E, ""),
}

# POST actions, by the last path segment (lower case)
ACTION = {
    "poweroff": (O, ""), "deallocate": (O, ""), "stop": (O, ""), "hibernate": (O, ""), "suspend": (O, ""),
    "restart": (E, ""), "reboot": (E, ""), "redeploy": (E, ""), "start": (A, ""), "resume": (A, ""),
    "reimage": (G, "wipes the OS disk"), "reimageall": (G, ""), "generalize": (O, "the VM can no longer start"),
    "performmaintenance": (E, ""), "simulateeviction": (O, ""),
    "regeneratekey": (O, "the old key stops"), "regeneratekeys": (O, ""), "regenerateaccesskey": (O, ""),
    "regeneratecredential": (O, ""), "regeneratecredentials": (O, ""), "regenerateautostoragekey": (O, ""),
    "resetserviceprincipalprofile": (X, ""), "resetaadprofile": (X, ""), "rotateclustercertificates": (E, ""),
    "rotateserviceaccountsigningkeys": (E, ""), "resetdbcredentials": (O, ""),
    "listkeys": (A, "reads a key"), "listcredentials": (A, "reads a credential"), "listsecrets": (A, ""),
    "listconnectionstrings": (A, ""), "listcredential": (A, ""), "listclusteradmincredential": (A, ""),
    "listclusterusercredential": (A, ""), "listclustermonitoringusercredential": (A, ""), "listaccountsas": (X, "makes a shared access signature"),
    "listservicesas": (X, "makes a shared access signature"), "beginGetAccess".lower(): (X, "makes a download URL for the disk"),
    "endgetaccess": (A, ""), "generatekey": (X, ""), "generatecredentials": (X, ""),
    "getuserdelegationkey": (X, ""), "revokeuserdelegationkeys": (O, ""),
    "purge": (G, ""), "failover": (E, ""), "forcefailover": (E, ""), "failoverpriority": (E, ""),
    "offlineregion": (O, ""), "onlineregion": (A, ""),
    "export": (E, ""), "exporttemplate": (A, ""), "import": (E, "overwrites"), "importimage": (A, ""),
    "restore": (E, "overwrites"), "restorefrombackupblob": (E, ""), "restorefromdeletedapp": (A, ""), "restoresnapshot": (E, ""),
    "moveresources": (E, ""), "validatemoveresources": (A, ""), "capture": (A, ""), "converttomanageddisks": (E, ""),
    "runcommand": (E, "runs a script on the VM"), "cancel": (A, ""), "abort": (A, ""), "abortlivemigration": (A, ""),
    "swap": (D, ""), "slotsswap": (D, ""), "applyslotconfig": (D, ""), "resetslotconfig": (E, ""),
    "syncfunctiontriggers": (A, ""), "syncfunctions": (A, ""), "backup": (A, ""), "discoverbackup": (A, ""),
    "publishxml": (A, "reads publish credentials"), "list": (A, ""), "listpublishingcredentials": (A, ""),
    "syncrepository": (D, ""), "migratemysql": (E, ""), "migrate": (E, ""), "upgrade": (E, ""),
    "approveordeny": (X, ""), "approve": (X, ""), "reject": (O, ""), "trigger": (A, ""), "run": (E, ""),
    "scheduleRun".lower(): (E, ""), "ping": (A, ""), "pause": (O, ""), "deletecontent": (G, ""),
    "cancelinitialreplication": (A, ""), "promote": (E, ""), "addhostname": (A, ""),
    "updatemachineimages": (D, ""), "manualupgrade": (D, ""), "updateinstances": (D, ""),
    "deleteinstances": (G, ""), "reapply": (A, ""), "installpatches": (E, ""), "assesspatches": (A, ""),
    "migratetovirtualmachinescaleset": (E, ""), "approverollingupgrade": (D, ""), "cancelrollingupgrade": (A, ""),
    "startosupgrade": (D, ""), "startextensionupgrade": (D, ""), "setorchestrationservicestate": (E, ""),
    "forcerecoveryservicefabricplatformupdatedomainwalk": (E, ""), "converttosingleplacementgroup": (E, ""),
    "retrievebootdiagnosticsdata": (A, ""), "attachdetachdatadisks": (E, ""), "migratetovmscale": (E, ""),
    "resetpassword": (X, ""), "resetkeys": (O, ""), "renamekey": (E, ""),
    "connect": (A, ""), "listnodes": (A, ""), "getaccess": (X, ""), "checkdomainavailability": (A, ""),
    "repair": (E, ""), "importfrommaster": (E, ""), "forcereboot": (E, ""), "flush": (G, "clears the cache"),
    "regenerate": (O, ""), "importdata": (E, ""), "exportdata": (E, ""),
}

# PUT or PATCH, by the resource type
WRITE = {
    "roleassignments": (X, "gives a role"), "federatedidentitycredentials": (X, "lets an outside identity sign in"),
    "firewallrules": (C, ""), "securityrules": (C, ""), "networksecuritygroups": (E, ""),
    "storageaccounts": (C, ""), "containers": (C, ""), "vaults": (C, ""), "accesspolicies": (X, ""),
    "registries": (C, ""), "tokens": (X, ""), "credentialsets": (X, ""), "scopemaps": (E, ""),
    "webhooks": (X, "sends events to a URL"), "replications": (A, ""),
    "managedclusters": (C, ""), "agentpools": (C, ""), "sites": (D, ""), "slots": (D, ""), "serverfarms": (E, ""),
    "config": (E, ""), "appsettings": (E, ""), "web": (E, ""), "authsettings": (C, ""), "authsettingsv2": (C, ""),
    "connectionstrings": (E, ""), "publishingcredentials": (E, ""), "basicpublishingcredentialspolicies": (X, ""),
    "ftp": (X, ""), "scm": (X, ""), "ipsecurityrestrictions": (X, ""), "hostnamebindings": (E, ""),
    "publicipaddresses": (E, ""), "locks": (A, "protects"), "policyassignments": (E, ""),
    "deployments": (C, ""), "resourcegroups": (A, ""), "databases": (E, ""), "servers": (C, ""),
    "flexibleservers": (C, ""), "databaseaccounts": (C, ""), "redis": (C, ""), "disks": (C, ""), "snapshots": (E, ""),
    "virtualmachines": (E, ""), "virtualmachinescalesets": (C, ""), "userassignedidentities": (A, ""),
    "roledefinitions": (E, ""), "administrators": (X, ""), "sqlroleassignments": (X, ""),
    "sqlroledefinitions": (E, ""), "mongouserdefinitions": (X, ""), "mongoroledefinitions": (E, ""),
    "privateendpointconnections": (E, ""), "blobservices": (C, ""), "fileservices": (E, ""),
    "managementpolicies": (C, ""), "default": (E, ""), "tags": (A, ""), "extensions": (E, ""), "runcommands": (E, "runs a script"),
    "configurations": (E, ""), "auditingsettings": (E, ""), "elasticpools": (E, ""), "keys": (X, "sets a function key"),
    "sourcecontrols": (D, ""), "sitecontainers": (D, ""), "functions": (D, ""), "certificates": (E, ""),
    "backuplongtermretentionpolicies": (E, ""), "backupshorttermretentionpolicies": (E, ""),
    "dnszones": (E, ""), "recordsets": (E, ""), "a": (E, ""), "aaaa": (E, ""), "cname": (E, ""), "txt": (E, ""),
    "mx": (E, ""), "ns": (E, ""), "srv": (E, ""), "caa": (E, ""), "ptr": (E, ""),
}

TYPE_RX = re.compile(r"\{[^}]+\}")


def resource_type(path):
    """The resource type a path ends in: the literal before the last {name}, or the last literal."""
    segs = path.strip("/").split("/")
    if TYPE_RX.fullmatch(segs[-1]):
        return segs[-2].lower(), None
    # a literal last segment: an action (POST) or a fixed name (default, web, appsettings)
    prev = segs[-2].lower() if len(segs) > 1 else ""
    return prev if not TYPE_RX.fullmatch(segs[-2]) else segs[-3].lower(), segs[-1]


def label(op):
    meth, path = op["method"], op["path"]
    low = path.lower()
    typ, last = resource_type(path)
    if "/tags" in low and ("/providers/microsoft.resources/tags" in low or low.endswith("/tagnames/{tagname}") or "/tagnames/" in low):
        return A, "tags"
    if meth == "DELETE":
        if last and last.lower() in DELETE:
            return DELETE[last.lower()]
        if typ in DELETE:
            return DELETE[typ]
        return O, "deletes a resource"
    if meth == "POST":
        act = (last or "").lower()
        if act in ACTION:
            return ACTION[act]
        if act.startswith(("list", "get", "check", "validate", "analyze", "preview", "query", "test", "search", "compute",
                           "calculate", "retrieve", "export")):
            return A, "reads"
        if act.startswith(("regenerate", "reset")):
            return O, ""
        if act.startswith(("delete", "purge", "remove")):
            return G if "backup" in act or "content" in act else O, ""
        if act.startswith(("stop", "disable", "revoke", "detach")):
            return O, ""
        if act.startswith(("start", "enable", "create", "add", "update", "sync", "refresh", "apply")):
            return E if act.startswith(("update", "apply")) else A, ""
        return E, "an action"
    # PUT / PATCH
    key = (last or typ).lower()
    if key in WRITE:
        return WRITE[key]
    if typ in WRITE:
        return WRITE[typ]
    return E, "creates or changes a resource"


def main():
    ops = json.load(open(os.path.join(HERE, "ops", "azure.json")))
    with open(os.path.join(HERE, "labels", "azure.txt"), "w") as f:
        f.write("# Azure Resource Manager (14 providers), from az 2.93.0 and its SDKs, labelled on 2026-11-25.\n"
                "# Key: METHOD path. By resource type and action (label_azure.py), every shape read.\n")
        for op in ops:
            lab, why = label(op)
            f.write("%s %s %s %s\n" % (op["method"], op["path"], lab, why))
    print("azure", len(ops))


if __name__ == "__main__":
    main()
