# Objective-Z Versioning

During pre-1.0 development, Objective-Z uses the following version scheme:

```
v0.SERIES.NN
```

**SERIES** increments on breaking changes — a shift in toolchain requirements, transpiler output contract, or PAL surface. It signals that consumers may need to adapt.

**NN** is the OZ issue watermark. Each resolved issue (whether a feature or a fix) increments this number. It is monotonic and never resets on a SERIES bump. It maps directly to the `issues/` directory: version `v0.5.87` means OZ-087 was the last resolved issue.

## VERSION file

The canonical version lives in `VERSION` at the repository root, following Zephyr's convention:

```
VERSION_MAJOR = 0
VERSION_MINOR = 5
PATCHLEVEL = 87
VERSION_TWEAK = 0
EXTRAVERSION =
```

## Rationale

Strict semver bumps MINOR on every new feature and resets PATCH to zero. At the current development velocity, this would inflate MINOR past 100 while the resetting PATCH loses all traceability. The OZ scheme preserves two things that matter pre-1.0: SERIES tells you if the architecture changed, and NN tells you exactly where the project stands relative to the issue tracker.

## Post-1.0

Once Objective-Z reaches 1.0, the project will adopt strict semver with a resetting PATCH segment. The OZ issue tracker will continue to provide traceability independently of the version string.
