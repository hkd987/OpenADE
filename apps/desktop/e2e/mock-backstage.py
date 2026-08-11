#!/usr/bin/env python3
"""Minimal mock Backstage for the e2e suite: catalog by-name/by-query and
TechDocs static content, mirroring the REST shapes catalog-mcp consumes."""
import json
from http.server import BaseHTTPRequestHandler, HTTPServer

PAYMENTS = {
    "apiVersion": "backstage.io/v1alpha1",
    "kind": "Component",
    "metadata": {
        "name": "payments-api",
        "namespace": "default",
        "title": "Payments API",
        "description": "Handles payment authorization and capture.",
        "tags": ["payments", "tier-1"],
        "links": [{"url": "https://example.com/runbook", "title": "Runbook"}],
    },
    "spec": {"type": "service", "lifecycle": "production", "owner": "payments-team"},
    "relations": [
        {"type": "ownedBy", "targetRef": "group:default/payments-team"},
        {"type": "dependsOn", "targetRef": "component:default/ledger"},
    ],
}
TEAM = {
    "apiVersion": "backstage.io/v1alpha1",
    "kind": "Group",
    "metadata": {
        "name": "payments-team",
        "namespace": "default",
        "title": "Payments Team",
        "links": [{"url": "https://chat.example.com/payments-eng"}],
    },
    "spec": {"type": "team"},
    "relations": [],
}
LEDGER = {
    "apiVersion": "backstage.io/v1alpha1",
    "kind": "Component",
    "metadata": {
        "name": "ledger",
        "namespace": "default",
        "description": "Double-entry ledger.",
    },
    "spec": {"type": "service"},
    "relations": [],
}


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def do_GET(self):
        if self.path == "/":
            body = b"mock backstage up"
            self.send_response(200)
            self.send_header("content-length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return
        routes = {
            "/api/catalog/entities/by-name/component/default/payments-api": PAYMENTS,
            "/api/catalog/entities/by-name/group/default/payments-team": TEAM,
            "/api/catalog/entities/by-name/component/default/ledger": LEDGER,
        }
        for prefix, entity in routes.items():
            if self.path.startswith(prefix):
                data = json.dumps(entity).encode()
                self.send_response(200)
                self.send_header("content-type", "application/json")
                self.send_header("content-length", str(len(data)))
                self.end_headers()
                self.wfile.write(data)
                return
        if self.path.startswith("/api/catalog/entities/by-query"):
            data = json.dumps({"items": [PAYMENTS, LEDGER]}).encode()
            self.send_response(200)
            self.send_header("content-type", "application/json")
            self.send_header("content-length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)
            return
        self.send_response(404)
        self.end_headers()


if __name__ == "__main__":
    HTTPServer(("127.0.0.1", 7998), Handler).serve_forever()
