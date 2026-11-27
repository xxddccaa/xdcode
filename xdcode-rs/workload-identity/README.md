# Workload identity assertions

This crate exchanges an assertion for a short-lived Provider access token. The
exchange caches the access token in memory and reacquires an assertion when it
needs to refresh. The login crate selects and pins the source from the process
environment.

File acquisition remains the default: set `PROVIDER_FEDERATION_RULE_ID` and an
absolute `PROVIDER_IDENTITY_TOKEN_FILE`. The file is reopened for every exchange.

For SPIFFE Workload API acquisition on Unix, configure the credential-owning
Xdcode process with:

```sh
export PROVIDER_FEDERATION_RULE_ID="<federation-rule-id>"
export PROVIDER_IDENTITY_TOKEN_SOURCE="spiffe"
export SPIFFE_ENDPOINT_SOCKET="unix:///run/spire/agent.sock"
export PROVIDER_SPIFFE_AUDIENCE="<audience-accepted-by-the-federation-rule>"
export PROVIDER_SPIFFE_ID="spiffe://example.org/workload"
```

`PROVIDER_SPIFFE_ID` is optional if the agent returns exactly one JWT-SVID.
`PROVIDER_IDENTITY_TOKEN_FILE` must be unset in SPIFFE mode. An ambient
`SPIFFE_ENDPOINT_SOCKET` alone does not select workload identity authentication.
TCP endpoints are not supported. Remote app-server deployments need the socket
on the host that owns authentication.

Xdcode calls the standard `FetchJWTSVID` RPC with the SPIFFE security metadata.
It keeps the JWT-SVID in memory for the exchange and reconnects on subsequent
acquisitions. Connection and request time are bounded together to ten seconds.
The token endpoint remains responsible for signature and lifetime verification.
An agent may reuse a JWT-SVID; deployments enabling assertion replay protection
must configure their issuer accordingly.

The agent's workload-attestation policy controls which processes can acquire
the identity. This feature does not add socket permissions to sandboxed tools
or make bearer tokens bound to a host. Provider-specific WIF settings are removed
from model-launched environments; the standard SPIFFE endpoint variable retains
its existing behavior for other workloads.
