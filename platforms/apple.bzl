"""Compatibility constraint shared by every Swift target in ios/."""

# Swift targets need an Apple host. Marking them incompatible on Linux lets a
# wildcard build on the Android box skip them instead of failing analysis.
APPLE_ONLY = select({
    "@platforms//os:linux": ["@platforms//:incompatible"],
    "//conditions:default": [],
})
