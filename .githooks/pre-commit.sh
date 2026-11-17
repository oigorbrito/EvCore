#!/bin/sh
# Blocks commits whose added lines look like an API key or token.
# A local safety net only: review the diff before committing anyway.
set -eu

PATTERN='(sk-or-[A-Za-z0-9_-]{20,}|sk-ant-[A-Za-z0-9_-]{20,}|sk-[A-Za-z0-9]{32,}|ghp_[A-Za-z0-9]{36}|github_pat_[A-Za-z0-9_]{50,}|AKIA[0-9A-Z]{16})'

if git diff --cached -U0 | grep -E '^\+[^+]' | grep -E -q "$PATTERN"; then
  echo "pre-commit: possível chave ou token nas linhas que você está commitando." >&2
  echo "Veja com 'git diff --cached', remova o valor e tente de novo." >&2
  echo "Falso positivo? Use 'git commit --no-verify' de propósito." >&2
  exit 1
fi