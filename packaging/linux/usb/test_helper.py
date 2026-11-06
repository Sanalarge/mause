#!/usr/bin/python3
import pathlib
import runpy
import types
import unittest
from unittest.mock import patch
import xml.etree.ElementTree as ET

HERE = pathlib.Path(__file__).resolve().parent
API = runpy.run_path(str(HERE / "mause-usb-helper"), run_name="usb_helper_tests")

class HelperTests(unittest.TestCase):
    def test_only_supported_operations_and_strict_identifiers_are_accepted(self):
        args = API["arguments"]
        self.assertEqual(args(["attach", "1-2.3", "04567"]), ("attach", "1-2.3", "4567"))
        for supplied in (["sh", "-c", "id"], ["bind", "1-1;reboot"], ["bind", "--help"],
                         ["unbind", "../../etc"], ["attach", "1-2", "0"], ["attach", "1-2", "65536"],
                         ["detach", "1-1", "extra"], ["attach", "1-2", "$(id)"], ["bind", "--"],
                         ["bind", "1..-2"], ["bind", "1-2..3"]):
            with self.subTest(supplied=supplied), self.assertRaises(ValueError):
                args(supplied)

    def test_detach_only_selects_the_exact_local_tunnel_device(self):
        imported = API["import_port"]
        text = "Port 00: <Port in Use>\n1-1 -> usbip://127.0.0.1:4567/1-2.3\n"
        self.assertEqual(imported(text, "1-2.3"), "00")
        with self.assertRaises(ValueError):
            imported(text, "1-2")
        with self.assertRaises(ValueError):
            imported(text.replace("127.0.0.1", "192.168.1.50"), "1-2.3")

    def test_export_never_starts_if_firewall_cannot_be_enforced(self):
        called = []
        def fake_run(name, *args, **kwargs):
            called.append(name)
            if name == "iptables":
                raise RuntimeError("firewall unavailable")
            return types.SimpleNamespace(returncode=0, stdout="")
        with patch.dict(API["operate"].__globals__, {"run": fake_run, "physical_device": lambda _: None}):
            with self.assertRaises(RuntimeError):
                API["operate"]("bind", "1-2")
        self.assertEqual(called, ["modprobe", "iptables"])

    def test_attach_always_uses_loopback_and_fixed_usbip_options(self):
        called = []
        def fake_run(name, *args, **kwargs):
            called.append((name, args))
            return types.SimpleNamespace(returncode=0, stdout="")
        with patch.dict(API["operate"].__globals__, {"run": fake_run}):
            API["operate"]("attach", "1-2", "4567")
        self.assertEqual(called[-1], ("usbip", ("--tcp-port", "4567", "attach", "-r", "127.0.0.1", "-b", "1-2")))

    def test_user_owned_or_writable_system_utilities_are_rejected(self):
        for uid, mode in [(1000, 0o755), (0, 0o777), (0, 0o775)]:
            with patch.object(pathlib.Path, "is_file", return_value=True), patch.object(pathlib.Path, "stat", return_value=types.SimpleNamespace(st_uid=uid, st_mode=mode)):
                with self.assertRaises(ValueError):
                    API["executable"]("usbip")

    def test_policy_is_limited_to_the_helper_and_defaults_to_deny(self):
        policy = ET.parse(HERE / "org.mause.usb.policy").getroot()
        actions = policy.findall("action")
        self.assertEqual(len(actions), 1)
        action = actions[0]
        self.assertEqual(action.attrib["id"], "org.mause.usb.manage")
        self.assertEqual(action.find("annotate").text, "/usr/libexec/mause-usb-helper")
        self.assertEqual([v.text for v in action.find("defaults")], ["no", "no", "no"])

if __name__ == "__main__":
    unittest.main()
