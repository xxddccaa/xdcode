import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";

import { afterEach, beforeEach } from "@jest/globals";

const originalXdcodeHome = process.env.XDCODE_HOME;
let currentXdcodeHome: string | undefined;

beforeEach(async () => {
  currentXdcodeHome = await fs.mkdtemp(path.join(os.tmpdir(), "xdcode-sdk-test-"));
  process.env.XDCODE_HOME = currentXdcodeHome;
});

afterEach(async () => {
  const xdcodeHomeToDelete = currentXdcodeHome;
  currentXdcodeHome = undefined;

  if (originalXdcodeHome === undefined) {
    delete process.env.XDCODE_HOME;
  } else {
    process.env.XDCODE_HOME = originalXdcodeHome;
  }

  if (xdcodeHomeToDelete) {
    await fs.rm(xdcodeHomeToDelete, { recursive: true, force: true });
  }
});
