#!/usr/bin/env bash
# Print the index digest and the per-architecture digests of a published
# multi-arch image as one JSON object on stdout:
#
#   {"image":"ghcr.io/xraph/octopus","version":"0.5.0",
#    "index_digest":"sha256:...","digests":{"amd64":"sha256:...","arm64":"sha256:..."}}
#
# Usage: scripts/image-digests.sh <image> <version>
#
# Exits non-zero, with a message on stderr, when the tag does not exist or when
# the index lacks a linux/amd64 or linux/arm64 manifest. Attestation manifests
# (platform unknown/unknown) are ignored.
set -euo pipefail

if [ "$#" -ne 2 ]; then
  echo "usage: $0 <image> <version>" >&2
  echo "example: $0 ghcr.io/xraph/octopus 0.5.0" >&2
  exit 2
fi

image="$1"
version="$2"

for tool in docker jq; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "error: required tool '$tool' not found in PATH" >&2
    exit 127
  fi
done

if ! manifest="$(docker buildx imagetools inspect "${image}:${version}" --format '{{json .Manifest}}')"; then
  echo "error: could not inspect ${image}:${version} (does the tag exist?)" >&2
  exit 1
fi

if ! jq -e '.digest and (.manifests | type == "array")' >/dev/null <<<"$manifest"; then
  echo "error: ${image}:${version} is not a multi-arch image index" >&2
  exit 1
fi

result="$(jq -c --arg image "$image" --arg version "$version" '
  def digest_for($arch):
    [.manifests[]
     | select(.platform.os == "linux" and .platform.architecture == $arch)
     | .digest][0];
  {
    image: $image,
    version: $version,
    index_digest: .digest,
    digests: {amd64: digest_for("amd64"), arm64: digest_for("arm64")}
  }' <<<"$manifest")"

for arch in amd64 arm64; do
  if ! jq -e --arg arch "$arch" '.digests[$arch] | type == "string"' >/dev/null <<<"$result"; then
    echo "error: ${image}:${version} has no linux/${arch} manifest" >&2
    exit 1
  fi
done

printf '%s\n' "$result"
