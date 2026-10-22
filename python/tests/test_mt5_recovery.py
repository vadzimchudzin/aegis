"""Automatic recovery of a broken MetaTrader 5 IPC link and the health report."""

import os
import sys
import unittest

for _path in (os.path.join(os.path.dirname(__file__), "fake_mt5"), os.path.join(os.path.dirname(__file__), "..")):
    # Insert once: the missing-package tests remove the fixture path a single time.
    if _path not in sys.path:
        sys.path.insert(0, _path)

import MetaTrader5 as fake  # noqa: E402

from aegis_lab.bridges import mt5_bridge  # noqa: E402
from aegis_lab.bridges.mt5_bridge import Bridge, BridgeError  # noqa: E402


class RecoveryTest(unittest.TestCase):
    def setUp(self):
        self.saved = dict(fake._state)
        fake._state.update(ipc_ok=True, connected=True, quote_age_seconds=0, initialize_calls=0)
        self.bridge = Bridge(fake)
        self.bridge.connect({"login": 1, "password": "good", "server": "RoboForex-ECN"})
        self.bridge._last_attempt = 0.0

    def tearDown(self):
        fake._state.clear()
        fake._state.update(self.saved)

    def break_ipc(self):
        fake._state["ipc_ok"] = False
        fake._state["error"] = (-10001, "IPC send failed")
        self.bridge._last_attempt = 0.0

    def test_candles_recover_after_ipc_send_failed(self):
        self.break_ipc()
        calls = fake._state["initialize_calls"]
        bars = self.bridge.candles({"timeframe": "1m", "limit": 3})
        self.assertEqual(len(bars), 3)
        self.assertEqual(fake._state["initialize_calls"], calls + 1)
        self.assertEqual(self.bridge._recovery["recovered"], 1)

    def test_trading_state_and_snapshot_recover(self):
        self.break_ipc()
        state = self.bridge.trading_state({})
        self.assertEqual(state["account"]["login"], 1)
        self.break_ipc()
        sample = self.bridge.market_snapshot({})
        self.assertEqual(sample["symbol"], "XAUUSD.r")

    def test_closed_terminal_reports_actionable_error_and_rate_limits(self):
        self.break_ipc()
        original = fake.initialize
        fake.initialize = lambda *a, **k: (fake._state.update(error=(-10003, "IPC initialize failed")), False)[1]
        try:
            with self.assertRaises(BridgeError) as error:
                self.bridge.trading_state({})
            self.assertIn("terminal is running", str(error.exception))
            # A second request within the interval does not hammer the terminal.
            with self.assertRaises(BridgeError) as again:
                self.bridge.trading_state({})
            self.assertIn("in progress", str(again.exception))
        finally:
            fake.initialize = original

    def test_recovery_refuses_a_different_account(self):
        self.break_ipc()
        original = fake.account_info

        def other():
            info = original()
            info.login = 99
            return info

        fake.account_info = other
        try:
            with self.assertRaises(BridgeError) as error:
                self.bridge.candles({"timeframe": "1m", "limit": 1})
            self.assertIn("different account", str(error.exception))
            self.assertIsNone(self.bridge.symbol)
        finally:
            fake.account_info = original

    def test_disconnected_terminal_is_explained(self):
        fake._state["connected"] = False
        with self.assertRaises(BridgeError) as error:
            self.bridge.market_snapshot({})
        self.assertIn("trade server", str(error.exception))

    def test_health_reports_link_quote_and_recovery(self):
        report = self.bridge.health()
        self.assertTrue(report["ipc"])
        self.assertTrue(report["terminal"]["connected"])
        self.assertTrue(report["rates_ok"])
        self.assertLess(abs(report["quote"]["age_ms"]), 2000)
        fake._state["quote_age_seconds"] = 45
        self.assertGreater(self.bridge.health()["quote"]["age_ms"], 40_000)
        self.break_ipc()
        report = self.bridge.health()
        self.assertTrue(report["ipc"])
        self.assertEqual(report["recovery"]["recovered"], 1)

    def test_clock_correction_shifts_app_time_only_within_bounds(self):
        before = self.bridge._now_ms()
        self.bridge.set_clock({"clock_correction_ms": -42_000})
        self.assertAlmostEqual(self.bridge._now_ms(), before - 42_000, delta=1000)
        with self.assertRaises(BridgeError):
            self.bridge.set_clock({"clock_correction_ms": mt5_bridge.MAX_CLOCK_CORRECTION_MS + 1})
        with self.assertRaises(BridgeError):
            self.bridge.set_clock({"clock_correction_ms": 1.5})

    def test_reinit_is_an_explicit_repair(self):
        calls = fake._state["initialize_calls"]
        report = self.bridge.reinit()
        self.assertTrue(report["ipc"])
        self.assertEqual(fake._state["initialize_calls"], calls + 1)

    def test_shutdown_forgets_the_login(self):
        self.bridge.shutdown()
        with self.assertRaises(BridgeError):
            self.bridge.reinit()


if __name__ == "__main__":
    unittest.main()
