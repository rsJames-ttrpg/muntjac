from setuptools import setup, Extension
setup(
    ext_modules=[Extension("synth._c", sources=["src/_c.c"])],
)
