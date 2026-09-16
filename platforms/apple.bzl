"""Compatibility constraint shared by every Swift target and Apple genrule."""

# Swift and Apple archive targets need an Apple host. Marking them incompatible
# on Linux and Android lets a wildcard build on the Android box skip them
# instead of failing analysis or running cargo for an Apple target.
APPLE_ONLY = select({
    "@platforms//os:linux": ["@platforms//:incompatible"],
    "@platforms//os:android": ["@platforms//:incompatible"],
    "//conditions:default": [],
})
