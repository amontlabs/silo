"""Labels for the Kubernetes API (v1.37.2 swagger): the label comes from the action
(x-kubernetes-action) and the resource or subresource the path names, from a table read by hand.
Writes labels/kubernetes.txt. Words as in label_aws.py.
"""
import json, os

HERE = os.path.dirname(os.path.abspath(__file__))
G, O, X, R, E, A, D, C, M = ("hold:gone", "hold:outage", "hold:exposed", "hold:recoverable", "either", "allow",
                             "deploy", "cond", "hold:money")

# delete of one object, by resource
DELETE = {
    "namespaces": (G, "everything in the namespace"), "persistentvolumeclaims": (G, "the volume goes if its reclaim policy is Delete"),
    "persistentvolumes": (G, ""), "secrets": (G, ""), "customresourcedefinitions": (G, "every object of the type goes"),
    "configmaps": (O, ""), "deployments": (O, ""), "statefulsets": (O, "its volumes stay"), "daemonsets": (O, ""),
    "replicasets": (O, ""), "replicationcontrollers": (O, ""), "jobs": (E, "a running job stops"), "cronjobs": (O, ""),
    "services": (O, ""), "ingresses": (O, ""), "ingressclasses": (O, ""), "nodes": (O, ""), "serviceaccounts": (O, ""),
    "roles": (O, ""), "rolebindings": (O, ""), "clusterroles": (O, ""), "clusterrolebindings": (O, ""),
    "networkpolicies": (X, "traffic it blocked is let through"),
    "validatingwebhookconfigurations": (X, "an admission check goes"), "mutatingwebhookconfigurations": (O, ""),
    "validatingadmissionpolicies": (X, "an admission check goes"), "validatingadmissionpolicybindings": (X, ""),
    "mutatingadmissionpolicies": (O, ""), "mutatingadmissionpolicybindings": (O, ""),
    "storageclasses": (O, ""), "csidrivers": (O, ""), "runtimeclasses": (O, ""), "apiservices": (O, ""),
    "volumeattachments": (O, ""), "priorityclasses": (E, ""), "poddisruptionbudgets": (E, "evictions are no longer limited"),
    "horizontalpodautoscalers": (E, ""), "resourcequotas": (E, "spending is no longer limited"), "limitranges": (E, ""),
    "pods": (E, "a controller recreates it, a bare pod is gone"), "endpoints": (E, ""), "endpointslices": (E, ""),
    "events": (A, ""), "leases": (A, ""), "leasecandidates": (A, ""), "controllerrevisions": (A, ""),
    "certificatesigningrequests": (A, ""), "podtemplates": (A, ""), "flowschemas": (E, ""),
    "prioritylevelconfigurations": (E, ""), "csinodes": (A, ""), "csistoragecapacities": (A, ""),
    "ipaddresses": (E, ""), "servicecidrs": (O, ""), "resourceclaims": (E, ""), "resourceclaimtemplates": (A, ""),
    "resourceslices": (A, ""), "deviceclasses": (E, ""), "devicetaintrules": (E, ""), "volumeattributesclasses": (A, ""),
    "storageversionmigrations": (A, ""), "storageversions": (A, ""), "clustertrustbundles": (O, ""),
    "podcertificaterequests": (A, ""), "podgroups": (E, ""), "compositepodgroups": (E, ""), "workloads": (E, ""),
    "evictionrequests": (A, ""), "resourcepoolstatusrequests": (A, ""),
}
# deletecollection: everything matching a selector in a namespace, or in the cluster
DELETECOLLECTION_UP = {"pods": (O, "every pod that matches"), "jobs": (O, ""), "events": (A, ""), "leases": (A, ""),
                       "endpoints": (E, ""), "endpointslices": (E, ""), "controllerrevisions": (A, ""),
                       "certificatesigningrequests": (A, ""), "podtemplates": (A, ""), "csinodes": (A, ""),
                       "csistoragecapacities": (A, ""), "resourceslices": (A, ""), "storageversionmigrations": (A, ""),
                       "storageversions": (A, ""), "podcertificaterequests": (A, ""), "evictionrequests": (A, ""),
                       "resourcepoolstatusrequests": (A, ""), "leasecandidates": (A, "")}
CREATE = {
    "rolebindings": (X, "gives a role"), "clusterrolebindings": (X, "gives a cluster role"),
    "roles": (E, ""), "clusterroles": (E, ""), "token": (X, "a token for the service account"),
    "exec": (E, "a shell in a container"), "attach": (E, ""), "portforward": (E, ""), "proxy": (E, ""),
    "eviction": (O, "evicts the pod"), "evictions": (O, ""), "approval": (X, "approves a certificate"),
    "services": (C, ""), "deployments": (D, ""), "statefulsets": (D, ""), "daemonsets": (D, ""),
    "pods": (A, ""), "secrets": (A, ""), "certificatesigningrequests": (E, ""),
    "validatingwebhookconfigurations": (E, ""), "mutatingwebhookconfigurations": (X, "rewrites objects as they are created"),
    "mutatingadmissionpolicies": (E, ""), "mutatingadmissionpolicybindings": (E, ""),
    "customresourcedefinitions": (A, ""), "apiservices": (X, "routes an API group to a service"),
    "finalize": (E, ""), "binding": (A, ""), "bindings": (A, ""), "ephemeralcontainers": (E, "a debug container in a pod"),
}
REPLACE = {  # put or patch
    "scale": (C, ""), "deployments": (C, ""), "statefulsets": (C, ""), "replicasets": (C, ""),
    "replicationcontrollers": (C, ""), "daemonsets": (D, ""), "services": (C, ""), "nodes": (C, ""),
    "rolebindings": (X, ""), "clusterrolebindings": (X, ""), "roles": (X, ""), "clusterroles": (X, ""),
    "secrets": (E, ""), "configmaps": (E, ""), "networkpolicies": (E, ""), "persistentvolumes": (E, "may set the reclaim policy"),
    "persistentvolumeclaims": (E, ""), "namespaces": (E, ""), "customresourcedefinitions": (E, "may drop a version"),
    "validatingwebhookconfigurations": (E, ""), "mutatingwebhookconfigurations": (E, ""),
    "validatingadmissionpolicies": (E, ""), "validatingadmissionpolicybindings": (E, ""),
    "serviceaccounts": (E, ""), "ingresses": (E, ""), "cronjobs": (E, ""), "jobs": (E, ""),
    "status": (A, "controllers' status"), "approval": (X, "approves a certificate"), "finalize": (E, ""),
    "ephemeralcontainers": (E, ""), "resize": (E, ""), "pods": (E, ""), "events": (A, ""), "leases": (A, ""),
    "leasecandidates": (A, ""), "endpoints": (E, ""), "endpointslices": (E, ""), "resourcequotas": (E, ""),
    "limitranges": (E, ""), "horizontalpodautoscalers": (E, ""), "poddisruptionbudgets": (E, ""),
    "apiservices": (X, ""), "priorityclasses": (E, ""), "storageclasses": (E, ""),
}


def resource(path):
    segs = path.strip("/").split("/")
    lits = [s for s in segs if not s.startswith("{")]
    return lits[-1]


def label(op):
    act, res = op["action"], resource(op["path"])
    if act == "delete":
        return DELETE.get(res, (O, "deletes an object"))
    if act == "deletecollection":
        if res in DELETECOLLECTION_UP:
            return DELETECOLLECTION_UP[res]
        lab, why = DELETE.get(res, (O, ""))
        return (G if lab == G else O if lab in (E, A) else lab), "every object that matches"
    if act == "connect":
        return CREATE.get(res, (E, ""))
    if act == "post":
        if res.endswith("reviews"):
            return A, "asks a question, changes nothing"
        return CREATE.get(res, (A, ""))
    return REPLACE.get(res, (E, ""))


def main():
    ops = json.load(open(os.path.join(HERE, "ops", "kubernetes.json")))
    with open(os.path.join(HERE, "labels", "kubernetes.txt"), "w") as f:
        f.write("# Kubernetes API (v1.37.2 swagger), every write operation, labelled on 2026-11-25.\n"
                "# Key: METHOD path. By action and resource (label_kubernetes.py).\n")
        for op in ops:
            lab, why = label(op)
            f.write("%s %s %s %s\n" % (op["method"], op["path"], lab, why))
    print("kubernetes", len(ops))


if __name__ == "__main__":
    main()
