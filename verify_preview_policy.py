"""HTTP boundary tests with fake control methods; never starts a USB process."""
import json
from http.server import ThreadingHTTPServer
import threading
import unittest
from unittest.mock import Mock
from urllib.request import Request, urlopen
from urllib.error import HTTPError
import floor_live_editor as app
import release_policy

class PreviewPolicy(unittest.TestCase):
    def setUp(self):
        self.server = ThreadingHTTPServer(('127.0.0.1', 0), app.Handler)
        self.server.manual_session = False
        self.server.experimental_device = False
        self.server.controls = Mock()
        self.server.stop_requested = False
        self.previous = app.HOST
        app.HOST = '127.0.0.1:' + str(self.server.server_port)
        self.origin = 'http://' + app.HOST
        self.addCleanup(setattr, app, 'HOST', self.previous)
        self.addCleanup(self.server.server_close)
        self.addCleanup(self.server.shutdown)
        threading.Thread(target=self.server.serve_forever, daemon=True).start()

    def request(self, path, body=None, origin=None):
        headers = {'Content-Type':'application/json', 'Origin':origin or self.origin}
        req = Request(self.origin + path, data=None if body is None else json.dumps(body).encode(), headers=headers)
        try:
            with urlopen(req, timeout=3) as r:
                return r.status, json.load(r)
        except HTTPError as e:
            return e.code, e.read()

    def test_default_refuses_device_connect_and_edit(self):
        code, policy = self.request('/live/capabilities')
        self.assertEqual(200, code)
        self.assertFalse(policy['deviceEnabled'])
        self.assertFalse(policy['transfersEnabled'])
        for path, body in [('/live/connect', {'manual':True}), ('/live/edit', {'op':'bypass'})]:
            self.assertEqual(409, self.request(path, body)[0])
        self.server.controls.start.assert_not_called()
        self.server.controls.edit.assert_not_called()

    def test_opt_in_still_blocks_every_sample_transfer_operation(self):
        self.server.experimental_device = True
        for operation in release_policy.BLOCKED_OPERATIONS:
            self.assertEqual(409, self.request('/live/edit', {'op':operation})[0])
        self.assertEqual(403, self.request('/live/ir/install', {})[0])
        self.server.controls.edit.assert_not_called()
        self.assertEqual(200, self.request('/live/edit', {'op':'bypass'})[0])
        self.server.controls.edit.assert_called_once_with({'op':'bypass'})

    def test_legacy_routes_are_unavailable(self):
        for path in ('/transfer','/current','/irs','/nam','/presets/0'):
            self.assertEqual(403, self.request(path)[0])
        for path in ('/recall/0/01A','/boost/add/1'):
            self.assertEqual(403, self.request(path,{})[0])
        self.assertEqual([], self.server.controls.mock_calls)

    def test_shutdown_checks_origin_and_releases_session(self):
        self.assertEqual(403, self.request('/live/shutdown', {}, 'http://example.invalid')[0])
        self.server.controls.stop.assert_not_called()
        self.assertEqual(200, self.request('/live/shutdown', {})[0])
        self.server.controls.stop.assert_called_once()
        self.assertTrue(self.server.stop_requested)

if __name__ == '__main__': unittest.main()
