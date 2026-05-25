import numpy as np
print("LEGACY numpy", np.__version__)
assert np.__version__.startswith("1."), np.__version__
