#!/usr/bin/env bash
set -euo pipefail

if [[ "$#" -ne 3 ]]; then
  echo "usage: validate_release_dispatch.sh <artifact-version> <create-release:true|false> <git-ref>" >&2
  exit 2
fi

version="$1"
create_release="$2"
git_ref="$3"

case "$create_release" in
  true | false) ;;
  *)
    echo "create-release must be true or false, got ${create_release}" >&2
    exit 2
    ;;
esac

workspace_version="$(awk -F '"' '/^version = "/ { print $2; exit }' Cargo.toml)"
if [[ -z "$workspace_version" ]]; then
  echo "Cargo.toml has no workspace package version" >&2
  exit 1
fi
if [[ "$version" != "$workspace_version" ]]; then
  echo "artifact-version ${version} does not match workspace version ${workspace_version}; dispatch the workflow with artifact_version=${workspace_version}" >&2
  exit 1
fi
if ! grep -Fq "## [${version}] - " CHANGELOG.md; then
  echo "CHANGELOG.md has no dated ${version} section; finalize that release section before dispatch" >&2
  exit 1
fi
if [[ "$create_release" == "true" && "$git_ref" != "refs/heads/master" ]]; then
  echo "public releases must be dispatched from refs/heads/master, got ${git_ref}; dispatch from master or set create_release=false" >&2
  exit 1
fi
