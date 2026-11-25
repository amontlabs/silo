"""Each test is a miss of the blind rules on the CLI captures, written before its fix.
RULES=blind runs them on rules-blind.json (they fail), the default runs them on rules.json.
Run: python3 -B -m pytest -q tests/   (from proto/)
"""
import json, os, sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, ".."))
sys.path.insert(0, os.path.join(HERE, "..", "capture"))
import score  # noqa: E402
import labels  # noqa: E402

DET = score.load(os.environ.get("RULES", "tuned"))
JSON = [["content-type", "application/json"], ["authorization", "Bearer <redacted>"]]


def req(host, method, target, body=b"", headers=JSON):
    return {"host": host, "method": method, "target": target, "headers": headers, "body": body, "facts": {}}


def test_az_deployment_complete_mode_with_az_s_invalid_json():
    # az 2.93 sends the template under an unquoted key: the body is not JSON, so "mode" cannot be read.
    # The blind rules took "cannot read" for "absent" and let it through.
    body = b'{"properties": {"parameters": {}, "mode": "Complete", template:{"resources":[]}\n}}'
    r = DET.check(req("management.azure.com", "PUT",
                      "/subscriptions/s/resourcegroups/rg1/providers/Microsoft.Resources/deployments/d1?api-version=2025-04-01", body))
    assert r["decision"] == "hold", r


def test_kubectl_protobuf_body_under_a_condition_is_not_absent():
    # kubectl 1.37 sends creates as protobuf; a condition on such a body cannot be read either
    body = b"k8s\x00\n\x0f\n\x02v1\x12\x07Service\x12..."
    r = DET.check(req("kube.capture.test", "POST", "/api/v1/namespaces/prod/services?fieldManager=kubectl-expose", body,
                      [["content-type", "application/vnd.kubernetes.protobuf"], ["authorization", "Bearer <redacted>"]]))
    assert r["decision"] == "hold", r


def test_gcs_bucket_iam_policy_is_the_body_itself():
    # Cloud Storage's JSON API takes the policy as the body, not under "policy"
    body = json.dumps({"bindings": [{"role": "roles/storage.objectViewer", "members": ["allUsers"]}], "etag": "BwX="}).encode()
    r = DET.check(req("storage.googleapis.com", "PUT", "/storage/v1/b/b1/iam?alt=json", body))
    assert r["decision"] == "hold" and r["verdict"] == "destructive", r


def test_gcloud_uses_resource_manager_v1():
    r = DET.check(req("cloudresourcemanager.googleapis.com", "DELETE", "/v1/projects/proj1?alt=json"))
    assert r["decision"] == "hold" and r["verdict"] == "destructive", r


def test_gcloud_uses_cloud_functions_v1():
    r = DET.check(req("cloudfunctions.googleapis.com", "DELETE", "/v1/projects/proj1/locations/us-central1/functions/f1?alt=json"))
    assert r["decision"] == "hold" and r["verdict"] == "destructive", r


def test_resource_manager_v1_public_binding():
    body = json.dumps({"policy": {"bindings": [{"role": "roles/viewer", "members": ["allUsers"]}]}}).encode()
    r = DET.check(req("cloudresourcemanager.googleapis.com", "POST", "/v1/projects/proj1:setIamPolicy?alt=json", body))
    assert r["decision"] == "hold", r


def test_az_webapp_stop():
    r = DET.check(req("management.azure.com", "POST",
                      "/subscriptions/s/resourceGroups/rg1/providers/Microsoft.Web/sites/app1/stop?api-version=2025-05-01"))
    assert r["decision"] == "hold" and r["verdict"] == "destructive", r


def test_google_operation_wait_is_a_read():
    e = {"method": "POST", "target": "/compute/v1/projects/proj1/zones/us-central1-a/operations/op-1/wait?alt=json", "headers": []}
    assert labels.is_read(e)
