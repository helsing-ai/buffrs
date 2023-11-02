## Continuous Integration

To utilize continuous integration for your Buffrs package (e.g. to automate code review and publishing
of your packages) you can utilize the following templates for GitHub Actions and GitLab CI:

### GitHub Actions

```yaml
name: Buffrs

on:
  push:
    branches:
      - '*'
  tags:
    - '*'

env:
  REGISTRY: https://<org>.jfrog.io/artifactoy
  REPOSITORY: your-artifactory-repo

jobs:
  verify:
    runs-on: ubuntu-latest

    steps:
      - name: Checkout code
        uses: actions/checkout@v2

      - name: Install Rust
        uses: actions-rs/toolchain@v1
        with:
          profile: minimal
          toolchain: nightly

      - name: Set up Rust environment
        run: |
          rustup target add aarch64-unknown-linux-gnu
        shell: bash

      - name: Verify
        run: |
          cargo install --force buffrs
          echo $TOKEN | buffrs login --registry $REGISTRY
          buffrs lint
        env:
          TOKEN: ${{ secrets.BUFFRS_TOKEN }}
        shell: bash

  publish:
    runs-on: ubuntu-latest
    needs: build
    if: startsWith(github.ref, 'refs/tags/')

    steps:
      - name: Checkout code
        uses: actions/checkout@v2

      - name: Publish on tag
        run: |
          cargo install --force buffrs
          echo $TOKEN | buffrs login --registry $REGISTRY
          buffrs publish --registry $REGISTRY --repository $REPOSITORY
        env:
          TOKEN: ${{ secrets.BUFFRS_TOKEN }}
        shell: bash

```

### GitLab CI

```yaml
stages:
  - verify
  - publish

variables:
  TOKEN: $BUFFRS_TOKEN  # Your secret artifactory token
  REGISTRY: https://<org>.jfrog.io/artifactory
  REPOSITORY: your-artifactory-repo

verify:
  stage: verify
  script:
    - cargo install buffrs
    - echo $TOKEN | buffrs login --registry $REGISTRY
    - buffrs lint
  only:
    - branches

publish:
  stage: publish
  script:
    - cargo install buffrs
    - echo $TOKEN | buffrs login --registry $REGISTRY
    - buffrs publish --registry $REGISTRY --repository $REPOSITORY
  only:
    - tags
```
