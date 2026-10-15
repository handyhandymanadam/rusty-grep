# PGO profile

The merged profile-guided-optimisation data (`harrier.profdata`) is not included in this repository;
generate it with:

    KEEP_PROFILE=/tmp/prof FEATURES=pcre2 scripts/build-pgo.sh /tmp/hw
    cp /tmp/prof/merged.profdata pgo/harrier.profdata

A stale profile only loses some of the speed-up, it does not break the build.
