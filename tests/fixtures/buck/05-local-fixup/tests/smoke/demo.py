"""Verifies the overlay genrule landed the marker module."""
import tomli
import tomli._overlay_marker

assert tomli._overlay_marker.OVERLAY_APPLIED, "overlay marker not loaded"
print("tomli imported; overlay marker imported; OVERLAY_APPLIED:", tomli._overlay_marker.OVERLAY_APPLIED)
