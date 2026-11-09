"""Regression tests for control extraction. Run with the demo's Python environment."""

import importlib.util
from pathlib import Path
import unittest

import numpy as np


spec = importlib.util.spec_from_file_location("throat_track", Path(__file__).with_name("throat-track.py"))
track = importlib.util.module_from_spec(spec)
spec.loader.exec_module(track)
focus_spec = importlib.util.spec_from_file_location("focus_throat", Path(__file__).with_name("focus-throat.py"))
focus = importlib.util.module_from_spec(focus_spec)
focus_spec.loader.exec_module(focus)


class ContinuousControls(unittest.TestCase):
    def test_high_focus_is_detected_without_a_separated_vocal_track(self):
        frequencies = np.arange(0, 8001, 8)
        power = np.ones((len(frequencies), 80)) * 0.001
        power += (np.exp(-0.5 * ((frequencies - 3000) / 25) ** 2) * 10)[:, None]
        centers, strength = focus.focus_contour(frequencies, power)
        np.testing.assert_allclose(centers, 3000, atol=2)
        self.assertGreater(strength.min(), 0.95)

    def test_broadband_sound_and_a_lower_instrument_harmonic_are_not_high_focus(self):
        frequencies = np.arange(0, 8001, 8)
        for power in [np.ones((len(frequencies), 80)),
                      np.repeat((0.001 + np.exp(-0.5 * ((frequencies - 1110) / 20) ** 2))[:, None], 80, axis=1)]:
            _, strength = focus.focus_contour(frequencies, power)
            self.assertLess(strength.max(), 0.05)

    def test_low_harmonic_tone_keeps_its_fractional_pitch(self):
        time = np.arange(800) / 8000
        for expected in [53.25, 74.25, 88.75, 110.4]:
            audio = sum(np.sin(2 * np.pi * expected * harmonic * time) / harmonic
                        for harmonic in range(1, 16))
            actual, confidence = track.pitch(audio, 8000, 45, 140)
            self.assertAlmostEqual(actual, expected, delta=0.1)
            self.assertGreater(confidence, 0.9)

    def test_temporary_harmonic_ambiguity_does_not_change_register(self):
        candidates = [np.array([[74.25, 0.1], [111.375, 0.5]]) for _ in range(100)]
        for i in range(40, 43):
            candidates[i] = np.array([[74.25, 0.3], [111.375, 0.1]])
        pitch, reliable = track.track_pitch(candidates, np.ones(100))
        self.assertTrue(reliable.all())
        np.testing.assert_allclose(pitch, 74.25, atol=0.01)

    def test_real_pitch_change_survives_continuity_constraint(self):
        candidates = [np.array([[74.25, 0.1], [88.75, 0.6]]) for _ in range(50)]
        candidates += [np.array([[74.25, 0.6], [88.75, 0.1]]) for _ in range(50)]
        pitch, _ = track.track_pitch(candidates, np.ones(100))
        self.assertAlmostEqual(pitch[20], 74.25, delta=0.01)
        self.assertAlmostEqual(pitch[80], 88.75, delta=0.01)

    def test_uncertain_pitch_does_not_interrupt_an_audible_phrase(self):
        rms = np.ones(100)
        candidates = [np.array([[74.25, 0.1]]) for _ in rms]
        for i in range(30, 36):
            candidates[i] = np.array([[74.25, 0.8]])
        pitch, reliable = track.track_pitch(candidates, rms)
        self.assertFalse(reliable[30:36].any())
        np.testing.assert_allclose(pitch, 74.25, atol=0.01)
        self.assertGreater(track.breath_envelope(rms).min(), 0.9)
        rms[60:85] = 0
        self.assertEqual(track.breath_envelope(rms)[70], 0)

    def test_formant_motion_blends_postures_instead_of_selecting_vowels(self):
        table = {
            "ɑ": {"formants": [500, 1200], "artic": {"jaw": 0.5}},
            "ə": {"formants": [500, 1600], "artic": {"jaw": 1.5}},
        }
        resonances = [np.array([500, f]) for f in np.linspace(1200, 1600, 100)]
        keys, positions = track.mouth_positions(table, resonances)
        self.assertEqual(keys, ["jaw"])
        self.assertGreater(len(np.unique(np.round(positions[:, 0], 3))), 80)
        self.assertLess(np.max(np.abs(np.diff(positions[:, 0]))), 0.02)


if __name__ == "__main__":
    unittest.main()
