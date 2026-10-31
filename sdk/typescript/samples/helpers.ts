import path from "node:path";

export function xdcodePathOverride() {
  return (
    process.env.XDCODE_EXECUTABLE ??
    path.join(process.cwd(), "..", "..", "xdcode-rs", "target", "debug", "xdcode")
  );
}
