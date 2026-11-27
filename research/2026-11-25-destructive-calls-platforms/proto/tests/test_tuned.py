"""The rules added after the captures hold what the CLIs sent and nothing next to it.

Run: python3 -m unittest discover -s tests   (from proto/)
"""
import json, os, sys, unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
import score  # noqa: E402

DET = score.load(score.NEW_RULES)
BLIND = score.load(score.BLIND_RULES)


def gql(query, variables=None, host="backboard.railway.com"):
    return {"host": host, "method": "POST", "target": "/graphql/v2", "headers": [["content-type", "application/json"]],
            "body": json.dumps({"query": query, "variables": variables or {}}).encode(), "facts": {}}


def rest(method, host, target, body=None):
    return {"host": host, "method": method, "target": target, "headers": [["content-type", "application/json"]],
            "body": json.dumps(body).encode() if body is not None else b"", "facts": {}}


PATCH = "mutation P($e: String!, $p: EnvironmentConfig!) { environmentPatchCommit(environmentId: $e, patch: $p) }"


class Tuned(unittest.TestCase):
    def check(self, req, want, blind=None):
        self.assertEqual(DET.check(dict(req))["decision"], want)
        if blind:
            self.assertEqual(BLIND.check(dict(req))["decision"], blind)

    def test_volume_detach(self):
        self.check(gql('mutation { volumeInstanceUpdate(input: { serviceId: null }, environmentId: "e", volumeId: "v") }'),
                   "hold", blind="allow")

    def test_volume_mount_path_change_allowed(self):
        self.check(gql('mutation { volumeInstanceUpdate(input: { mountPath: "/data" }, environmentId: "e", volumeId: "v") }'), "allow")

    def test_volume_attach_allowed(self):
        self.check(gql('mutation { volumeInstanceUpdate(input: { serviceId: "s" }, environmentId: "e", volumeId: "v") }'), "allow")

    def test_scale_region_removed(self):
        p = {"services": {"s1": {"deploy": {"multiRegionConfig": {"us-west2": None}}}}}
        self.check(gql(PATCH, {"e": "e", "p": p}), "hold", blind="allow")

    def test_scale_region_to_zero(self):
        p = {"services": {"s1": {"deploy": {"multiRegionConfig": {"us-west2": {"numReplicas": 0}}}}}}
        self.check(gql(PATCH, {"e": "e", "p": p}), "hold")

    def test_scale_up_allowed(self):
        p = {"services": {"s1": {"deploy": {"multiRegionConfig": {"us-west2": {"numReplicas": 3}}}}}}
        self.check(gql(PATCH, {"e": "e", "p": p}), "allow")

    def test_storage_object_delete(self):
        self.check(rest("DELETE", "abcdefghijklmnopqrst.supabase.co", "/storage/v1/object/avatars", {"prefixes": ["u1.png"]}),
                   "hold", blind="allow")

    def test_storage_object_read_allowed(self):
        self.check(rest("GET", "abcdefghijklmnopqrst.supabase.co", "/storage/v1/object/avatars/u1.png"), "allow")

    def test_vercel_unversioned_member_add(self):
        self.check(rest("POST", "api.vercel.com", "/teams/team_1/members", {"email": "bob@example.com"}), "hold", blind="allow")

    def test_vercel_unversioned_read_allowed(self):
        self.check(rest("GET", "api.vercel.com", "/teams/team_1/members"), "allow")


if __name__ == "__main__":
    unittest.main()
