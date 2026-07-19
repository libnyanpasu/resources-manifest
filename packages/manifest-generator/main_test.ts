import { assertEquals } from "@std/assert";
import { RESOURCE_MANIFEST_VERSION } from "@manifest/metadata";

Deno.test("targets the metadata manifest version", () => {
  assertEquals(RESOURCE_MANIFEST_VERSION, 2);
});
