// A plain ref name (git check-ref-format's rules, one-level names allowed) or
// a hex object id. Revision expressions (HEAD~1, ref^{tree}, @{u}, a..b,
// rev:path) are refused, so callers pass git exactly a ref or a SHA.
function isRefName(value) {
  if (
    value.length > 255 ||
    value.startsWith("-") ||
    value === "@" ||
    /[\x00-\x20\x7f~^:?*[\\]|\.\.|@\{|\/\/|^\/|\/$|\.$/.test(value)
  )
    return false;
  return value
    .split("/")
    .every(part => !part.startsWith(".") && !part.endsWith(".lock"));
}

export function assertRevision(value, name) {
  if (typeof value !== "string" || !value || !isRefName(value)) {
    throw Error(
      `${name} must be a git ref name or commit SHA, not an option, whitespace or a revision expression (got ${JSON.stringify(value)}). Next: use origin/main or an explicit commit SHA.`,
    );
  }
  return value;
}
