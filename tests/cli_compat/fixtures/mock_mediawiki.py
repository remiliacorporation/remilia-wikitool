#!/usr/bin/env python3
"""Loopback-only MediaWiki API fixture for the offline CLI compatibility gate."""

from __future__ import annotations

import argparse
import json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import parse_qs, urlsplit


NAMESPACES = {
    "0": {"id": 0, "name": "", "canonical": "", "content": ""},
    "8": {"id": 8, "name": "MediaWiki", "canonical": "MediaWiki"},
    "10": {"id": 10, "name": "Template", "canonical": "Template"},
    "14": {"id": 14, "name": "Category", "canonical": "Category"},
    "828": {"id": 828, "name": "Module", "canonical": "Module"},
}


class MediaWikiFixtureHandler(BaseHTTPRequestHandler):
    server_version = "WikitoolMediaWikiFixture/1"

    def do_GET(self) -> None:  # noqa: N802 - BaseHTTPRequestHandler contract
        request = urlsplit(self.path)
        if request.path != "/api.php":
            self.send_error(404)
            return

        params = parse_qs(request.query, keep_blank_values=True)
        self._write_json(self._api_response(params))

    def log_message(self, _format: str, *_args: object) -> None:
        return

    def _api_response(self, params: dict[str, list[str]]) -> dict[str, object]:
        action = self._first(params, "action")
        if action != "query":
            return {
                "error": {
                    "code": "fixture-unsupported-action",
                    "info": f"unsupported fixture action: {action}",
                }
            }

        if self._first(params, "meta") == "siteinfo":
            return self._siteinfo_response(self._first(params, "siprop"))
        if self._first(params, "list") == "allpages":
            pages = (
                [{"ns": 0, "title": "Delete Test"}]
                if self._first(params, "apnamespace") == "0"
                else []
            )
            return {"batchcomplete": True, "query": {"allpages": pages}}
        if self._first(params, "prop") == "revisions":
            title = self._first(params, "titles") or ""
            if title == "Delete Test":
                return {
                    "batchcomplete": True,
                    "query": {
                        "pages": [
                            {
                                "pageid": 1,
                                "ns": 0,
                                "title": title,
                                "revisions": [
                                    {
                                        "revid": 101,
                                        "timestamp": "2026-08-30T00:00:00Z",
                                        "comment": "offline fixture",
                                        "slots": {
                                            "main": {"content": "test content\n"}
                                        },
                                    }
                                ],
                            }
                        ]
                    },
                }
            return {
                "batchcomplete": True,
                "query": {
                    "pages": [
                        {
                            "ns": 0,
                            "title": title,
                            "missing": True,
                        }
                    ]
                },
            }
        return {
            "error": {
                "code": "fixture-unsupported-query",
                "info": "unsupported fixture query",
            }
        }

    @staticmethod
    def _siteinfo_response(siprop: str | None) -> dict[str, object]:
        if siprop == "extensiontags":
            query: dict[str, object] = {"extensiontags": ["ref", "syntaxhighlight"]}
        elif siprop == "functionhooks":
            query = {"functionhooks": ["if", "invoke"]}
        elif siprop == "magicwords":
            query = {
                "magicwords": [
                    {
                        "name": "redirect",
                        "aliases": ["#REDIRECT"],
                        "case-sensitive": False,
                    }
                ]
            }
        else:
            query = {
                "general": {
                    "articlepath": "/index.php?title=$1",
                    "generator": "MediaWiki 1.44.0",
                },
                "namespaces": NAMESPACES,
                "extensions": [
                    {"name": "Scribunto", "version": "1.44.0", "type": "parserhook"}
                ],
                "specialpagealiases": [],
            }
        return {"batchcomplete": True, "query": query}

    @staticmethod
    def _first(params: dict[str, list[str]], key: str) -> str | None:
        values = params.get(key)
        return values[0] if values else None

    def _write_json(self, payload: dict[str, object]) -> None:
        encoded = json.dumps(payload, separators=(",", ":")).encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(encoded)))
        self.end_headers()
        self.wfile.write(encoded)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--ready-file", required=True, type=Path)
    args = parser.parse_args()

    server = ThreadingHTTPServer(("127.0.0.1", 0), MediaWikiFixtureHandler)
    host, port = server.server_address
    ready_tmp = args.ready_file.with_suffix(f"{args.ready_file.suffix}.tmp")
    ready_tmp.write_text(f"http://{host}:{port}/api.php\n", encoding="utf-8")
    ready_tmp.replace(args.ready_file)
    server.serve_forever()


if __name__ == "__main__":
    main()
