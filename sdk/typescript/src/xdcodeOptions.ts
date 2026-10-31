export type XdcodeConfigValue = string | number | boolean | XdcodeConfigValue[] | XdcodeConfigObject;

export type XdcodeConfigObject = { [key: string]: XdcodeConfigValue };

export type XdcodeOptions = {
  xdcodePathOverride?: string;
  baseUrl?: string;
  apiKey?: string;
  /**
   * Additional `--config key=value` overrides to pass to the Xdcode CLI.
   *
   * Provide a JSON object and the SDK will flatten it into dotted paths and
   * serialize values as TOML literals so they are compatible with the CLI's
   * `--config` parsing.
   */
  config?: XdcodeConfigObject;
  /**
   * Raw `--config key=value` overrides to pass unchanged to the Xdcode CLI after
   * structured configuration and before SDK-managed or thread-specific overrides.
   */
  configOverrides?: string[];
  /**
   * Environment variables passed to the Xdcode CLI process. When provided, the SDK
   * will not inherit variables from `process.env`.
   */
  env?: Record<string, string>;
};
