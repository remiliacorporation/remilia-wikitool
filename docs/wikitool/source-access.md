# Source Access Sessions

Some source websites serve browser access challenges before readable content. Wikitool treats this
as a source-access outcome, not as article evidence. It does not ship stealth clients, TLS
fingerprint impersonation, paid crawl routes, or third-party reader proxies.

When `wikitool source fetch URL --output json` returns `error.challenge_handoffs`, follow the
handoff explicitly:

1. Open the URL in a normal browser where you have lawful access.
2. Solve the source challenge.
3. Export or copy the source-issued cookies.
4. Import them with the `suggested_argv` from the handoff, usually:

```bash
wikitool source session import "URL" --cookies - --user-agent "UA" --ttl-seconds 1800 --format json
```

5. Paste the cookie payload on stdin and close stdin.
6. Retry the fetch with `--refresh`.

Cookie input must come from stdin (`--cookies -`) or an existing regular, non-symlink file. The
payload may use Netscape `cookies.txt`, JSON, or raw `Cookie` header syntax. Never place cookie
values directly in `--cookies`: literal values are rejected without being echoed in diagnostics.
Imported sessions live under `.wikitool/source/sessions/`. CLI list/show output reports only
domains, cookie names, expiry, and paths; it never prints cookie values.

## Local Storage Security

On Windows, Wikitool applies a protected DACL to the session directory and every session file. The
DACL contains exactly one full-control entry for the current process user's SID, and Wikitool reads
the descriptor back to verify the owner SID, protection flag, entry count, access mask, inheritance
flags, and entry SID. It uses the native Windows security API and does not invoke `icacls` or
another subprocess. On Unix, it applies and verifies mode `0700` on the directory and `0600` on
each file, and verifies that the owner is the current effective user.

Existing session storage is brought under the same policy before Wikitool reads it. If the DACL
or Unix permissions cannot be applied or verified, import and loading fail closed. The empty atomic
staging file is secured and verified before cookie bytes are written, then verified again after
publication. A newly written session file whose file-level protection cannot be verified is removed
before the error is returned. Errors identify the storage operation and path, never cookie values.

## Bookmarklet Helper

This optional bookmarklet copies a simple JSON handoff for cookies visible to JavaScript:

```javascript
javascript:(async()=>{const data={url:location.href,ua:navigator.userAgent,cookies:document.cookie,ts:new Date().toISOString()};await navigator.clipboard.writeText(JSON.stringify(data,null,2));alert("Copied wikitool session handoff JSON");})();
```

Browser JavaScript cannot read `HttpOnly` cookies. If a challenge cookie is `HttpOnly`, use a
browser cookie export tool that produces Netscape `cookies.txt`, or copy the browser's request
`Cookie` header from developer tools. Only import cookies for sources you are permitted to access.

## Lifecycle

```bash
wikitool source session list --format json
wikitool source session show example.com --format json
wikitool source session clear example.com --format json
wikitool source session prune --format json
```

Matching sessions are used automatically by `source fetch`, live MediaWiki template inspection,
and `export`. The source-document cache key does not include cookies; cookies affect
access, not source identity. If an earlier unauthenticated fetch failed, retry with `--refresh`
after importing the session.
