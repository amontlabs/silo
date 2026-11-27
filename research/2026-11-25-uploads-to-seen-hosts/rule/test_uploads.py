"""Tests of the rule on requests written by hand: each surface, each credential form, and the ways
a hostile program would try to make an upload to someone else read as one to you.

    python3 -m unittest test_uploads.py
"""

import base64
import json
import unittest

from uploads import Accounts, classify, credential, decide

ACC = Accounts(github=["polarzero"], buckets=["polarzero-builds"], identities=["polarzero@example.com"],
               placeholders=["SILO_PLACEHOLDER_1"])
WHO = {"ghp_p": "polarzero", "ghp_m": "mallory", "ya29.p": "polarzero@example.com", "ya29.m": "m@x.net"}


def req(method, host, target, headers=(), body=b""):
    return {"case": "t", "method": method, "host": host, "target": target, "headers": list(headers),
            "body_b64": base64.b64encode(body).decode(), "bytes_up": 0, "bytes_down": 0}


def basic(user, pw):
    return ("Authorization", "Basic " + base64.b64encode(f"{user}:{pw}".encode()).decode())


def one(r):
    out = list(decide([r], ACC, lambda f, v, h: WHO.get(v), lambda n: n.split("_")[1] if n.count("_") >= 2 else None))
    _, kind, place, form, who, by_place, by_both = out[0]
    return kind, place, by_place, by_both


class Credentials(unittest.TestCase):
    def test_fixed_user_name_is_not_the_credential(self):
        self.assertEqual(credential(req("POST", "github.com", "/x", [basic("x-access-token", "ghp_m")]), {}),
                         ("basic", "ghp_m"))

    def test_token_as_user_name_is_the_credential(self):
        self.assertEqual(credential(req("POST", "github.com", "/x", [basic("ghp_m", "")]), {}), ("basic", "ghp_m"))

    def test_signed_url_names_its_signer(self):
        r = req("PUT", "storage.googleapis.com", "/b/o?X-Goog-Credential=sa%40p.iam.gserviceaccount.com%2F2026")
        self.assertEqual(credential(r, {})[0], "signed-url")

    def test_placeholder_is_silos(self):
        r = req("POST", "api.github.com", "/repos/polarzero/app/issues", [("Authorization", "token SILO_PLACEHOLDER_1")])
        self.assertEqual(one(r)[2:], ("allow", "allow"))


class Places(unittest.TestCase):
    def test_push_to_someone_else(self):
        r = req("POST", "github.com", "/mallory/drop.git/git-receive-pack", [basic("x-access-token", "ghp_p")])
        self.assertEqual(one(r)[2:], ("refuse", "refuse"))

    def test_fetch_is_a_read(self):
        self.assertEqual(classify(req("POST", "github.com", "/mallory/drop.git/git-upload-pack"))[0], "read")

    def test_owner_case_does_not_matter(self):
        r = req("POST", "github.com", "/PolarZero/app.git/git-receive-pack", [basic("x", "ghp_p")])
        self.assertEqual(one(r)[2:], ("allow", "allow"))

    def test_lfs_upload_is_placed(self):
        r = req("POST", "github.com", "/mallory/drop.git/info/lfs/objects/batch", [basic("x", "ghp_m")],
                b'{"operation":"upload","objects":[]}')
        self.assertEqual(one(r)[1], "github:mallory")

    def test_issue_in_your_repository_written_by_someone_else(self):
        r = req("POST", "api.github.com", "/repos/polarzero/app/issues", [("Authorization", "token ghp_m")], b"{}")
        self.assertEqual(one(r)[2:], ("allow", "refuse"))

    def test_graphql_mutation_by_node_id(self):
        body = json.dumps({"query": "mutation CommentCreate($input: AddCommentInput!){addComment(input:$input){x}}",
                           "variables": {"input": {"subjectId": "I_mallory_drop_1", "body": "loot"}}}).encode()
        r = req("POST", "api.github.com", "/graphql", [("Authorization", "bearer ghp_p")], body)
        self.assertEqual(one(r)[1:], ("github:mallory", "refuse", "refuse"))

    def test_graphql_query_is_a_read(self):
        body = json.dumps({"query": "query { viewer { login } }"}).encode()
        self.assertEqual(classify(req("POST", "api.github.com", "/graphql", body=body))[0], "read")

    def test_gist_is_filed_under_the_credential(self):
        r = req("POST", "api.github.com", "/gists", [("Authorization", "token ghp_m")], b"{}")
        self.assertEqual(one(r)[1:3], ("gist:mallory", "refuse"))

    def test_gcs_copy_names_the_destination(self):
        r = req("POST", "storage.googleapis.com",
                "/storage/v1/b/polarzero-builds/o/secret/rewriteTo/b/mallory-drop/o/x",
                [("Authorization", "Bearer ya29.p")])
        self.assertEqual(one(r)[1:3], ("gcs:mallory-drop", "refuse"))

    def test_gcs_object_name_with_slashes_is_not_an_evasion(self):
        r = req("POST", "storage.googleapis.com",
                "/storage/v1/b/polarzero-builds/o/dist%2Fapp.tar/rewriteTo/b/polarzero-builds/o/dist%2Fold.tar",
                [("Authorization", "Bearer ya29.p")])
        self.assertEqual(one(r)[1:], ("gcs:polarzero-builds", "allow", "allow"))

    def test_gcs_virtual_host(self):
        r = req("PUT", "mallory-drop.storage.googleapis.com", "/x")
        self.assertEqual(one(r)[1:3], ("gcs:mallory-drop", "refuse"))

    def test_unknown_write_is_held(self):
        r = req("POST", "github.com", "/session", [("Cookie", "user_session=abc")])
        self.assertEqual(one(r)[2:], ("hold", "hold"))


class Evasions(unittest.TestCase):
    """A path the rule reads one way and the host may read another must not pass as yours."""

    def test_encoded_slash_in_owner(self):
        r = req("POST", "api.github.com", "/repos/polarzero%2F..%2Fmallory/drop/issues",
                [("Authorization", "token ghp_p")], b"{}")
        self.assertNotEqual(one(r)[2], "allow")

    def test_dot_segments(self):
        r = req("POST", "github.com", "/polarzero/../mallory/drop.git/git-receive-pack", [basic("x", "ghp_p")])
        self.assertNotEqual(one(r)[2], "allow")

    def test_double_slash(self):
        r = req("PUT", "storage.googleapis.com", "//mallory-drop/x", [("Authorization", "Bearer ya29.p")])
        self.assertNotEqual(one(r)[1], "gcs:polarzero-builds")
        self.assertNotEqual(one(r)[2], "allow")

    def test_encoded_destination_bucket(self):
        r = req("POST", "storage.googleapis.com",
                "/storage/v1/b/polarzero-builds/o/x/rewriteTo/b/polarzero-builds%2F..%2Fmallory-drop/o/y",
                [("Authorization", "Bearer ya29.p")])
        self.assertNotEqual(one(r)[2], "allow")

    def test_method_override_header(self):
        r = req("GET", "api.github.com", "/repos/mallory/drop/issues",
                [("Authorization", "token ghp_p"), ("X-HTTP-Method-Override", "POST")])
        self.assertNotEqual(one(r)[2], "allow")


if __name__ == "__main__":
    unittest.main()
