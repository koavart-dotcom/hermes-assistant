"""Unit tests for hermes_camera (run: python3 -m unittest discover -s hermes-plugins/hermes_camera -p 'test_*.py')."""

import importlib.util
import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest import mock

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("hermes_camera", HERE / "__init__.py")
cam = importlib.util.module_from_spec(spec)
spec.loader.exec_module(cam)

LISTING = """[dshow @ 000001] "A4ech FHD 1080P PC Camera" (video)
[dshow @ 000001]   Alternative name "@device_pnp_\\\\?\\usb#vid_09da&pid_2704"
[dshow @ 000001] "Микрофон (A4ech FHD 1080P PC Camera)" (audio)
[dshow @ 000001] "OBS Virtual Camera" (video)
dummy: Immediate exit requested
"""


class DeviceTests(unittest.TestCase):
    def test_parse_devices_video_only(self):
        self.assertEqual(cam.parse_devices(LISTING), ["A4ech FHD 1080P PC Camera", "OBS Virtual Camera"])

    def test_pick_device(self):
        devs = cam.parse_devices(LISTING)
        self.assertEqual(cam.pick_device(devs, None), "A4ech FHD 1080P PC Camera")
        self.assertEqual(cam.pick_device(devs, "obs"), "OBS Virtual Camera")
        self.assertIsNone(cam.pick_device(devs, "logitech"))
        self.assertIsNone(cam.pick_device([], None))


class SnapshotTests(unittest.TestCase):
    def test_disabled_by_env(self):
        with mock.patch.dict(os.environ, {"HERMES_CAMERA": "off"}):
            self.assertFalse(cam.snapshot()["ok"])

    def test_handler_returns_json_and_never_raises(self):
        with mock.patch.object(cam, "snapshot", side_effect=RuntimeError("boom")):
            out = json.loads(cam._handle({"reason": "test"}))
        self.assertFalse(out["ok"])
        self.assertIn("boom", out["error"])

    def test_bundled_ffmpeg_preferred(self):
        with tempfile.TemporaryDirectory() as d:
            exe = Path(d) / "tools" / "ffmpeg-9.0.1-win32-x64" / "bin" / "ffmpeg.exe"
            exe.parent.mkdir(parents=True)
            exe.write_bytes(b"")
            self.assertEqual(cam.find_ffmpeg(Path(d)), str(exe))

    def test_prune_keeps_newest(self):
        with tempfile.TemporaryDirectory() as d:
            folder = Path(d)
            for i in range(13):
                p = folder / f"cam-2026101{i:02d}.jpg"
                p.write_bytes(b"x")
                os.utime(p, (1000 + i, 1000 + i))
            cam._prune(folder, keep=10)
            left = sorted(p.name for p in folder.glob("cam-*.jpg"))
            self.assertEqual(len(left), 10)
            self.assertNotIn("cam-202610100.jpg", left)

    def test_register_declares_tools(self):
        ctx = mock.Mock()
        cam.register(ctx)
        names = [c.kwargs["name"] for c in ctx.register_tool.call_args_list]
        self.assertEqual(names, ["camera_snapshot", "screen_snapshot"])
        self.assertTrue(all(c.kwargs["toolset"] == "camera" for c in ctx.register_tool.call_args_list))


class ScreenTests(unittest.TestCase):
    def test_window_target_uses_title(self):
        args, what = cam.screen_args("ffmpeg", Path("o.png"), "window", "Книга1 - Excel")
        self.assertIn("title=Книга1 - Excel", args)
        self.assertEqual(what, "окно «Книга1 - Excel»")

    def test_own_window_or_no_title_means_desktop(self):
        for title in ("Hermes Assistant", ""):
            args, what = cam.screen_args("ffmpeg", Path("o.png"), "window", title)
            self.assertIn("desktop", args)
            self.assertEqual(what, "весь экран")
        args, _ = cam.screen_args("ffmpeg", Path("o.png"), "screen", "Книга1 - Excel")
        self.assertIn("desktop", args)

    def test_screen_handler_never_raises(self):
        with mock.patch.object(cam, "screen", side_effect=RuntimeError("x")):
            self.assertFalse(json.loads(cam._handle_screen({}))["ok"])


if __name__ == "__main__":
    unittest.main()
