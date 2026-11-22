# ADR-032: Request IDs and distributed tracing - the request ID is the W3C trace ID

| Field | Value |
| --- | --- |
| **Status** | Accepted |
| **Date** | 2026-11-22 |
| **Deciders** | @ICreateThunder |
| **Tags** | observability, tracing, logging, request-id, opentelemetry |
| **Supersedes** | (none - refines [ADR-004 §B](ADR-004-defence-in-depth.md)) |

## Context

[ADR-004 §B](ADR-004-defence-in-depth.md) specifies structured JSON logs with one span per request and `x-request-id` propagation, "generated at ingress if absent". Two things have changed since:

- **A client must not choose the request ID.** The ID appears in operational logs, in problem+json error bodies ([ADR-006 §F](ADR-006-api-contract.md)) and on error pages, and support uses it to find a user's request. Accepting an inbound value lets a client choose what our records say. The ID must always be created on our side, with an inbound value kept only as a sanitised `client_request_id` log field.
- **Requests cross several hops.** In the hosted deployment a request passes Cloudflare, a load balancer, the Istio ingress gateway and the application, which calls PostgreSQL. A request ID that exists only in the application cannot follow a request through the gateway, and a trace that exists only in the gateway cannot be matched to a user's support reference.

W3C Trace Context defines a `traceparent` header carrying a 128-bit trace ID, a parent span ID and a sampled flag. Istio's gateway (Envoy) can start and propagate traces in this format. Istio ambient mode's ztunnel works at layer 4 and adds no HTTP spans; waypoint proxies, where deployed, do. VictoriaTraces accepts spans over OTLP (`/insert/opentelemetry/v1/traces`) and can be queried by `trace_id`; VictoriaLogs stores the JSON logs, and the two are linked in Grafana through a derived field on `trace_id`.

## Decision

**The request ID is the W3C trace ID of the request's trace. It is created by the first HTTP hop we operate, never by the client; the application trusts an incoming `traceparent` only from that hop; traces are exported over OTLP only to an endpoint the operator configures.**

### A. One identifier

- The request ID shown to users, written to `ProblemDetails.request_id`, and echoed in the `x-request-id` response header is the trace ID, as 32 lowercase hexadecimal characters.
- Every log line emitted inside a request carries `trace_id` and `span_id`.
- A support reference therefore leads directly to the trace in the tracing backend and to the matching log lines.
- Records that link back to a request, such as audit records, may store this ID. Whether each does, and in which column and type, is decided by the ADR that defines that record; this ADR does not change any record's schema.

The trace ID is 128 random bits and carries no timestamp, unlike the UUID v7 used until this decision is implemented.

### B. Who starts the trace

| Deployment | First HTTP hop we operate | Starts the trace |
| --- | --- | --- |
| Hosted | Istio ingress gateway | The gateway |
| Self-host without a mesh | The application | The application (OpenTelemetry SDK, random 128-bit trace ID) |

### C. Trust in incoming trace headers

- At the edge, a client's `traceparent` and `tracestate` are removed before the request reaches the gateway (a Cloudflare request-header rule in the hosted deployment). An incoming `traceparent` would let a client choose the trace ID, and its sampled flag would let a client force every request to be traced, turning tracing cost into a resource-exhaustion vector.
- The application reads `traceparent` according to the setting `FA_TRACEPARENT_TRUST` (flag `--traceparent-trust`):

| Setting | Behaviour | Used by |
| --- | --- | --- |
| `none` | Ignore any incoming `traceparent`; start a new trace | The default when the setting is unset; self-host |
| `upstream` | Continue the incoming trace | Set explicitly in hosted manifests, only when the preconditions below hold |

- If the setting is unset, it is `none`: the binary cannot tell where it is deployed, so it never trusts an incoming `traceparent` unless told to.
- `upstream` is used only when all of these hold in the deployment:
  1. **No path to the gateway bypasses Cloudflare.** Cloudflare runs in SSL mode Full (strict), every hop from Cloudflare to the load balancer and on to the Istio gateway is mutually authenticated, and the gateway accepts connections only with Cloudflare's client certificate. No direct load-balancer-to-gateway path is exposed.
  2. **Cloudflare removes client `traceparent` and `tracestate`** before forwarding.
  3. **Only the gateway reaches the application**: an Istio `AuthorizationPolicy` admits only the gateway's workload identity.

  Condition 1 is the control that stops a client reaching Envoy with its own `traceparent`, which Envoy would otherwise continue, sampled flag included. Condition 2 removes the headers on the path that remains. If a deployment cannot verify all three, it uses `none`, and the application starts every trace itself.
- An inbound `x-request-id` is never the request ID under either setting; a sanitised copy is logged as `client_request_id`.

### D. Export and storage

- Spans are produced with `tracing` and exported by `tracing-opentelemetry` over OTLP/HTTP, to an OpenTelemetry Collector or directly to VictoriaTraces.
- Database calls appear as child spans of the request, so query time is visible per request.
- Logs stay JSON on stdout, with `trace_id` and `span_id` added, and are shipped by the operator's log agent to VictoriaLogs or any other store.
- **No export happens unless the operator sets an OTLP endpoint.** The endpoint is configured with the standard OpenTelemetry variables `OTEL_EXPORTER_OTLP_ENDPOINT` and `OTEL_EXPORTER_OTLP_HEADERS`, as [ADR-001 §H](ADR-001-platform.md) specifies; when they are unset, spans are dropped. Traces go only to infrastructure the operator runs.

### E. Sampling

- Head sampling is decided where the trace starts (the gateway, or the application on self-host), at a configured ratio. The application follows the parent's sampled flag under `upstream`.
- Keeping every trace that ends in an error needs tail sampling, done in an OpenTelemetry Collector. That is deployment configuration, not application behaviour.
- Logs are not sampled: every request's log lines carry its trace ID whether or not its spans were sampled.

### F. Data in spans

Span attributes follow the same rules as logs ([ADR-004 §B](ADR-004-defence-in-depth.md)): route templates, never raw paths or query strings; opaque identifiers, never names, email addresses or other personal data; no request or response bodies. Retention in the tracing backend is set by the operator and documented with log retention.

## Consequences

### Positive

- One identifier connects a user's support reference, the error they saw, the log lines, the distributed trace and any record that stores it (§A).
- A client cannot choose the ID or force sampling.
- Self-hosted installs get the same behaviour without a mesh, and export nothing unless configured.

### Negative

- The OpenTelemetry crates (`opentelemetry`, `opentelemetry_sdk`, `opentelemetry-otlp`, `tracing-opentelemetry`) are added to the binary.
- `upstream` depends on deployment configuration outside this repository (origin mutual TLS, the edge header rule, the gateway-only `AuthorizationPolicy`); §C requires falling back to `none` when it cannot be verified.
- Linking logs and traces depends on a Grafana derived field; VictoriaMetrics lists automatic correlation as future work.

### Neutral

- The request-ID format changes from UUID v7 to a 32-character hexadecimal trace ID. No external consumer stores request IDs yet.

## Alternatives considered

### Alternative - keep a separate UUID v7 request ID and record it on the trace

Rejected: support would need a lookup from request ID to trace ID, and two identifiers would appear in every record.

### Alternative - let Envoy's `x-request-id` be the request ID

Rejected: it does not exist on self-hosted installs without a mesh, and it is a separate value from the trace ID unless Envoy is configured to derive one from the other.

### Alternative - trust any incoming `traceparent`

Rejected: a client could choose the trace ID and force sampling (§C).

## References

- [ADR-001 §H](ADR-001-platform.md) - no telemetry; OTLP endpoint variables.
- [ADR-004 §B](ADR-004-defence-in-depth.md) - structured logging; refined here.
- [ADR-006 §F](ADR-006-api-contract.md) - problem+json `request_id`.
- [ADR-021](ADR-021-cdn-front-door.md) - Cloudflare edge, Istio ambient runtime.
- W3C Trace Context: <https://www.w3.org/TR/trace-context/>
- VictoriaTraces documentation: <https://docs.victoriametrics.com/victoriatraces/>
