# Release Process

1. Update the version in `Cargo.toml` and `CHANGELOG.md`.
2. Commit the release preparation:
   ```bash
   git commit -am "chore(release): vX.Y.Z"
   ```
3. Create and push a signed or annotated version tag:
   ```bash
   git tag -a vX.Y.Z -m "Release vX.Y.Z"
   git push origin main --tags
   ```
4. GitHub Actions (`.github/workflows/release.yml`) will automatically build binaries for Linux, macOS, and Windows, generate `SHA256SUMS.txt`, and publish the GitHub Release.
