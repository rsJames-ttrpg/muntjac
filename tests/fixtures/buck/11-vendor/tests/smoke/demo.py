"""Smoke test for the 11-vendor fixture.

Imports the two vendored packages and prints their versions to prove the
committed wheels under third-party/python/vendor/ are wired into the
python_binary via the pypi_package macro's `vendor:` branch.
"""

import idna
import iniconfig

print("idna:", idna.__version__)
# iniconfig 2.0.0 does not expose `__version__`; touching a public attribute
# is enough to prove the wheel imported cleanly.
print("iniconfig:", iniconfig.IniConfig.__name__)
