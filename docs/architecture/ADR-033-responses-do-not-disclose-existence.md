# ADR-033: Responses do not disclose existence - 404 for anything the caller cannot see

| Field | Value |
| --- | --- |
| **Status** | Accepted |
| **Date** | 2026-11-23 |
| **Deciders** | @ICreateThunder |
| **Tags** | security, api, authorisation, privacy |
| **Supersedes** | (none - refines the `403` for a tenant mismatch in [ADR-001 §F](ADR-001-platform.md) and [ADR-006 §C](ADR-006-api-contract.md)) |

## Context

[ADR-006 §C](ADR-006-api-contract.md) puts the tenant in the path and says the `{tenant}` segment "must match the key's tenant or the request is `403`". [ADR-001 §F](ADR-001-platform.md) gives the same answer when a subdomain does not match the session's tenant claim.

The tenant routes followed that rule by looking up the slug first and then asking the policy whether the caller could use it. An unknown slug returned `404`, and another tenant's slug returned `403`. Any authenticated caller could therefore test slugs one at a time and learn which organisations use the service. The same pattern applied to another user's records or to special-category fields would disclose that those records exist, which for medical or safety data is itself personal information.

The status code is not the only signal. A handler that loads a row and then refuses it does more work than one that finds nothing: it runs the policy, and for encrypted fields it may unwrap a key or decrypt before deciding. The difference shows in response times.

RFC 9110 §15.5.4 allows the alternative directly:

> An origin server that wishes to "hide" the current existence of a forbidden target resource MAY instead respond with a status code of 404 (Not Found).

## Decision

**A resource the caller cannot see is not found. Visibility is applied inside the query that loads the resource, so an invisible resource and a missing one take the same path and produce the same response. `403` is used only for a resource the caller can see and an action the caller may not take on it.**

### A. Status codes

- `404 Not Found` for any resource owned by a controller the caller cannot see: another tenant, another tenant's records, or another user's records. The problem+json body ([ADR-006 §F](ADR-006-api-contract.md)) is identical whether or not the resource exists, apart from per-request values such as the request ID.
- Fields of a class the caller may not read are left out of a resource the caller can see; the resource stays visible. A route that addresses such a field on its own is a `404` under this rule.
- `403 Forbidden` after a lookup only when the caller can already see the resource and lacks the right for the requested action. A member reading their own tenant may see it; the same member renaming it without the tenant-admin role gets `403`.
- A credential without the scope for a route is still `403` ([ADR-006 §G](ADR-006-api-contract.md)), and `401 Unauthorized` is unchanged. Both are decided from the credential and the route before any lookup, so neither says anything about resources.
- A tenant in the path or subdomain that is not the caller's is a `404`, in place of the `403` in ADR-001 §F and ADR-006 §C. This holds for every layer that answers on our behalf: a check at the edge that rejects a mismatched tenant answers with the same `404` as the application.

### B. Visibility in the query

- The query that loads a resource carries the visibility condition, either as a predicate (for example the caller's tenant in the `WHERE` clause) or through row-level security. A forbidden lookup and a lookup of something that does not exist run the same statements, and both return no row.
- No decryption, key unwrap or secondary query happens before visibility is established.
- A subject with no context for the resource's controller sees nothing. On the product API that subject is never a staff actor: staff credentials do not authenticate on the product plane ([ADR-010 §A](ADR-010-platform-operator-access.md), [§B](ADR-010-platform-operator-access.md); [ADR-013](ADR-013-auth-keys.md)).
- Policies then decide actions on visible resources. A policy never receives a resource the caller cannot see, so a policy Deny always means `403`.

### C. Recording

- A lookup with a visibility condition that returns no row is a Deny, and is audited as [ADR-009 §B](ADR-009-event-streams-and-retention.md) requires for every Deny. The row goes to the caller's chain: their tenant chain, or their user chain when there is no tenant context. It never goes to the target's chain, because the target may not exist and naming it would need the lookup this ADR hides.
- The row is the same whether or not the target exists, so both cases do the same work before the response is sent. If recording is limited under load, the limit applies to both cases alike.
- Nothing on the response path works out which case it was. Telling "exists but not visible" from "does not exist" is optional, and happens only afterwards, for example when an operator investigates a request by its ID.
- A policy Deny on a visible resource (`403`) is recorded on the caller's chain in the same way.

## Consequences

### Positive

- An authenticated caller can no longer test which tenant slugs, records or special-category fields exist.
- The response time of a refused lookup matches that of a lookup for something missing, because both run the same statement and stop at the same point.
- Later resources follow one rule: put visibility in the query, then apply the policy.

### Negative

- A caller who mistypes a slug and a caller asking for a tenant they do not belong to get the same answer. Telling them apart takes an investigation by request ID.
- Every not-found lookup with a visibility condition writes an audit row, so those responses include a chain append.
- Client developers lose a signal: a `404` may mean "ask for access" as well as "does not exist". Documentation for each route has to say so.
- Every lookup must take the caller's context. A query helper that loads by key alone cannot be used on a request path.

### Neutral

- Route paths, the problem+json format and `401` handling are unchanged.

## Alternatives considered

### Alternative - keep 403 for a resource that exists but is not the caller's

The status quo. Rejected: it discloses existence, as described in the Context.

### Alternative - answer 403 for every refused or missing resource

Hides existence as well, but reports a typo inside the caller's own tenant as a refusal, using the code RFC 9110 defines for a request the server understood and will not fulfil. Rejected.

### Alternative - load first, check afterwards, and pad the response time

Keeps today's code shape and adds a delay to equalise timing. Rejected: the delay has to track every change to the work done after loading, decryption included, and any miss reopens the timing difference. A predicate in the query removes the difference instead of hiding it.

## References

### Related ADRs

- [ADR-001 §F](ADR-001-platform.md) - tenant context from the subdomain; mismatch now `404`.
- [ADR-006 §C](ADR-006-api-contract.md) - tenant in the path; mismatch now `404`. [§F](ADR-006-api-contract.md) - the problem+json envelope.
- [ADR-009 §B](ADR-009-event-streams-and-retention.md) - every Deny is audited.
- [ADR-010 §B](ADR-010-platform-operator-access.md) - actor classes; staff do not use the product API.

### External documentation

- RFC 9110: HTTP Semantics, [§15.5.4 403 Forbidden](https://www.rfc-editor.org/rfc/rfc9110#section-15.5.4) and [§15.5.5 404 Not Found](https://www.rfc-editor.org/rfc/rfc9110#section-15.5.5).
