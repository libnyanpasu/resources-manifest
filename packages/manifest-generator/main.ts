import { RESOURCE_MANIFEST_VERSION } from "@manifest/metadata";

export function main(): void {
  console.log(`manifest-generator (manifest v${RESOURCE_MANIFEST_VERSION})`);
}

if (import.meta.main) {
  main();
}
