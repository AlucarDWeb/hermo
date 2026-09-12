"""Repository rule providing the JNA AAR for its native dispatch libraries.

Adapted from just_nutrients_official_app tools/signing/jna_aar_repo.bzl
(READ-ONLY reference). rules_jvm_external resolves net.java.dev.jna:jna to a
jvm_import of the PLAIN JAR — the AAR's jni/<abi>/libjnidispatch.so never
reaches lib/<abi>/ in the APK. JNA on Android then fails both load paths:
System.loadLibrary (no lib/ entry) and the classpath resource
/com/sun/jna/android-aarch64/libjnidispatch.so (the JAR only ships linux-*
prefixes) → UnsatisfiedLinkError on the first UniFFI call.

This hermo version downloads the AAR directly from Maven Central (pinned by
sha256) instead of fishing it out of the coursier cache: coursier fetches the
plain jna jar (pom packaging=jar), so the AAR is never in the cache, and the
typed "g:a:aar:v" coordinate that would fetch it is rejected by coursier
when other roots are present.
"""

JNA_AAR_URL = "https://repo1.maven.org/maven2/net/java/dev/jna/jna/{version}/jna-{version}.aar"
JNA_AAR_SHA256 = {
    "5.13.0": "3a03daf5a7212201c3a9b44f734dfc9babb3addcb5cbfa639f9f535e887925c0",
}

def _jna_aar_impl(ctx):
    version = ctx.attr.version
    url = JNA_AAR_URL.format(version = version)
    sha256 = JNA_AAR_SHA256.get(version)
    if not sha256:
        fail("No pinned sha256 for jna " + version +
             "; add it to android/app/jna_aar_repo.bzl")
    ctx.download(url, "jna-%s.aar" % version, sha256 = sha256)
    ctx.file("BUILD", 'exports_files(["jna-%s.aar"])\n' % version)

jna_aar = repository_rule(
    implementation = _jna_aar_impl,
    attrs = {
        "version": attr.string(mandatory = True),
    },
)
