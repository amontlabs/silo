"""The pieces of the gateway live check that can be tested without a namespace."""
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('gateway_live_check', Path(__file__).with_name('gateway-live-check.py'))
check = importlib.util.module_from_spec(spec)
spec.loader.exec_module(check)


class UsageLogTests(unittest.TestCase):
    def test_json_lines_are_events(self):
        text = '{"v":1,"kind":"used","secret":"A","host":"api.github.com","method":"GET"}\n\n{"v":1,"kind":"blocked"}\n'
        events, malformed = check.parse_usage_log(text)
        self.assertEqual([event["kind"] for event in events], ['used', 'blocked'])
        self.assertEqual(malformed, 0)

    def test_a_line_that_is_not_a_json_object_is_counted_not_skipped(self):
        events, malformed = check.parse_usage_log('{"kind":"used"}\nnot json\n[1]\n"text"\n')
        self.assertEqual(len(events), 1)
        self.assertEqual(malformed, 3)

    def test_a_host_named_inside_another_field_is_not_a_use_of_it(self):
        # Counting substrings would have taken this for a use of example.com.
        events, _ = check.parse_usage_log('{"kind":"used","secret":"A","host":"api.github.com","method":"GET","note":"example.com"}\n')
        self.assertEqual([event["host"] for event in events], ['api.github.com'])


if __name__ == '__main__':
    unittest.main()
