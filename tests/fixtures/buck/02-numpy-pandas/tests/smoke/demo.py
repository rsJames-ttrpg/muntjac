import numpy as np

arr = np.zeros(3)
print(arr)
assert arr.shape == (3,), "unexpected shape {}".format(arr.shape)
