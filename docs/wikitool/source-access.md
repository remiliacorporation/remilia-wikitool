# Source Access Sessions

Some source websites serve browser access challenges before readable content. Wikitool treats this
as a source-access outcome, not as article evidence. It does not ship stealth clients, TLS
fingerprint impersonation, paid crawl routes, or third-party reader proxies.

When `wikitool research fetch URL --output json` returns `error.challenge_handoffs`, follow the
handoff explicitly:

1. Open the URL in a normal browser where you have lawful access.
2. Solve the source challenge.
3. Export or copy the source-issued cookies.
4. Import them with the `suggested_argv` from the handoff, usually:

```bash
wikitool research session import "URL" --cookies - --user-agent "UA" --ttl-seconds 1800 --format json
```

5. Paste the cookie payload on stdin and close stdin.
6. Retry the fetch with `--refresh`.

Cookie input may be a Netscape `cookies.txt` file, JSON, a raw `Cookie` header, or stdin. Imported
sessions live under `.wikitool/research/sessions/`. CLI list/show output reports only domains,
cookie names, expiry, and paths; it never prints cookie values.

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
wikitool research session list --format json
wikitool research session show example.com --format json
wikitool research session clear example.com --format json
wikitool research session prune --format json
```

Matching sessions are used automatically by `research fetch`, live MediaWiki template inspection,
and `export`. The research document cache key does not include cookies; cookies affect
access, not source identity. If an earlier unauthenticated fetch failed, retry with `--refresh`
after importing the session.
