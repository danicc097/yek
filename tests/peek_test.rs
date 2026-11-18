use assert_cmd::Command;
use std::fs;
use tempfile::tempdir;
use yek::{config::YekConfig, peek::peek_file, serialize_repo};

#[test]
fn test_peek_json_truncation() {
    let short_json = (1..=20)
        .map(|i| format!("  \"k{i}\": {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(peek_file("data.json", &short_json), short_json);

    let long_json = (1..=30)
        .map(|i| format!("  \"k{i}\": {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let peeked = peek_file("fixtures/big.json", &long_json);
    let lines: Vec<&str> = peeked.lines().collect();
    assert_eq!(lines.len(), 21);
    assert_eq!(lines[0], "  \"k1\": 1");
    assert_eq!(lines[19], "  \"k20\": 20");
    assert_eq!(lines[20], "[truncated...]");
}

#[test]
fn test_peek_cpp_header_and_source() {
    let header = r#"
#ifndef WIDGET_H
#define WIDGET_H
#pragma once
#include <QWidget>
#include "helper.h"

namespace app {

enum class Mode {
    Fast, ///< fast mode
    Accurate
};

class Widget : public QWidget {
    Q_OBJECT
    Q_PROPERTY(QString title READ title WRITE setTitle)
public:
    explicit Widget(QWidget* parent = nullptr);
    ~Widget() override = default;
    QString title() const { return m_title; }
    void setTitle(const QString& t);

signals:
    void changed(int count);

private:
    QString m_title;
    void internalHelper() { int x = 1; }
};

} // namespace app

#endif
"#;

    let out = peek_file("src/widget.h", header);
    assert!(!out.contains("#pragma once"));
    assert!(!out.contains("#include"));
    assert!(!out.contains("WIDGET_H"));
    assert!(!out.contains("Q_OBJECT"));
    assert!(!out.contains("Q_PROPERTY"));
    assert!(!out.contains("return m_title"));
    assert!(!out.contains("internalHelper"));
    assert!(out.contains("namespace app {"));
    assert!(out.contains("enum class Mode {"));
    assert!(out.contains("class Widget : public QWidget {"));
    assert!(out.contains("QString title() const;"));
    assert!(out.contains("void setTitle(const QString& t);"));
    assert!(out.contains("signals:"));
    assert!(out.contains("void changed(int count);"));

    let cpp = r#"
#include "widget.h"

namespace app {

template <class T>
T identity(T val) {
    return val;
}

Widget::Widget(QWidget* parent)
    : QWidget(parent)
    , m_title{"default"}
{
    setObjectName(QStringLiteral("w"));
}

void Widget::setTitle(const QString& t) {
    if (m_title == t) {
        return;
    }
    m_title = t;
}

} // namespace app
"#;

    let cpp_out = peek_file("src/widget.cpp", cpp);
    assert!(!cpp_out.contains("#include"));
    assert!(!cpp_out.contains("setObjectName"));
    assert!(!cpp_out.contains("m_title = t"));
    assert!(cpp_out.contains("template <class T> T identity(T val);"));
    assert!(cpp_out.contains("Widget::Widget(QWidget* parent);"));
    assert!(cpp_out.contains("void Widget::setTitle(const QString& t);"));
}

#[test]
fn test_peek_go_interfaces_structs_and_funcs() {
    let go_src = r#"package watchlist

import (
	"context"
	"net/http"
)

// Store manages watchlist entries.
type Store interface {
	Get(ctx context.Context, id string) (*Item, error)
	Save(ctx context.Context, item *Item) error
}

type Item struct {
	ID    string `json:"id"`
	Title string `json:"title"`
}

var BigLookup = map[string]string{
	"a": "alpha",
	"b": "beta",
}

// NewStore creates a store.
func NewStore(client *http.Client) *Store {
	if client == nil {
		client = http.DefaultClient
	}
	return nil
}

func (i *Item) Summary() interface{} {
	return map[string]string{"id": i.ID}
}

func asmFast(x int) int
"#;

    let out = peek_file("watchlist/store.go", go_src);
    assert!(out.contains("package watchlist"));
    assert!(!out.contains("import"));
    assert!(out.contains("type Store interface {"));
    assert!(out.contains("Get(ctx context.Context, id string) (*Item, error)"));
    assert!(out.contains("type Item struct {"));
    assert!(out.contains("ID string `json:\"id\"`"));
    assert!(out.contains("var BigLookup = map[string]string{...}"));
    assert!(!out.contains("\"alpha\""));
    assert!(out.contains("NewStore creates a store") && out.contains("func NewStore(client *http.Client) *Store"));
    assert!(!out.contains("DefaultClient"));
    assert!(out.contains("func (i *Item) Summary() interface{}"));
    assert!(out.contains("func asmFast(x int) int"));
}

#[test]
fn test_peek_python_classes_decorators_and_sigs() {
    let py_src = r#"from __future__ import annotations
import os

MAX_ITEMS = 100

@dataclass
class Config:
    """Configuration model."""
    host: str
    port: int = 8080
    tags: list[str] = [
        "a",
        "b",
    ]

    # Fetch endpoint
    @property
    def url(self) -> str:
        return f"http://{self.host}:{self.port}"

    async def fetch(
        self,
        path: str,
        timeout: float = 5.0,
    ) -> bytes:
        """
Multi-line docstring starting at column 0!
"""
        def inner_helper():
            return 1
        return b"ok"

def top_level(x: int) -> int: return x + 1

if __name__ == "__main__":
    print(top_level(1))
"#;

    let out = peek_file("app/config.py", py_src);
    assert!(!out.contains("import os"));
    assert!(!out.contains("MAX_ITEMS"));
    assert!(!out.contains("inner_helper"));
    assert!(!out.contains("print(top_level"));
    assert!(!out.contains("return x + 1"));
    assert!(out.contains("@dataclass"));
    assert!(out.contains("class Config:"));
    assert!(out.contains("    host: str"));
    assert!(out.contains("    port: int = 8080"));
    assert!(out.contains("    tags: list[str] = ..."));
    assert!(out.contains("    # Fetch endpoint"));
    assert!(out.contains("    @property"));
    assert!(out.contains("    def url(self) -> str: ..."));
    assert!(out.contains("    async def fetch("));
    assert!(out.contains("    ) -> bytes: ..."));
    assert!(out.contains("def top_level(x: int) -> int: ..."));
}

#[test]
fn test_peek_unsupported_extension_unchanged() {
    let md = "# Title\n\nSome markdown content.\n";
    assert_eq!(peek_file("README.md", md), md);
    let rs = "fn main() { println!(\"hi\"); }";
    assert_eq!(peek_file("src/main.rs", rs), rs);
}

#[test]
fn test_serialize_repo_with_peek_and_cli_flag() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("main.go"),
        "package main\n\nfunc hello() string {\n\treturn \"secret_body\"\n}\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("notes.txt"),
        "keep full text file content",
    )
    .unwrap();
    let big_json = (1..=25)
        .map(|i| format!("  \"line_{i}\": {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(dir.path().join("data.json"), &big_json).unwrap();

    let mut cfg = YekConfig::default();
    cfg.input_paths = vec![dir.path().to_string_lossy().to_string()];
    cfg.peek = true;

    let (output, _) = serialize_repo(&cfg).unwrap();
    assert!(output.contains("func hello() string"));
    assert!(!output.contains("secret_body"));
    assert!(output.contains("keep full text file content"));
    assert!(output.contains("[truncated...]"));
    assert!(!output.contains("line_25"));

    // Also verify the --peek CLI flag end-to-end
    let cmd_out = Command::cargo_bin("yek")
        .unwrap()
        .arg("--peek")
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(cmd_out.status.success());
    let stdout = String::from_utf8(cmd_out.stdout).unwrap();
    assert!(stdout.contains("func hello() string"));
    assert!(!stdout.contains("secret_body"));
    assert!(stdout.contains("[truncated...]"));
}
