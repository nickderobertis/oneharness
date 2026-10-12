export function assertRevision(value, name) {
  if (
    typeof value !== "string" ||
    !value ||
    value.startsWith("-") ||
    /[\x00-\x20\x7f]/.test(value)
  ) {
    throw Error(
      `${name} must be a nonempty git ref or SHA, not an option or whitespace. Next: use origin/main or an explicit commit SHA.`,
    );
  }
  return value;
}
