# protofish2-rs

## How to handle "Bump version"
1. Read commit since latest tag, and determine the type of version bump (major, minor, patch) and changes.
2. **Get an approval** from the user to proceed with the version bump.
3. If approved, update the version in `Cargo.toml` and update CHANGELOG.md with the new version and changes.
4. Commit the changes with a message like "chore: bump version to x.y.z" and tag the commit with the new version.
5. Don't push.
