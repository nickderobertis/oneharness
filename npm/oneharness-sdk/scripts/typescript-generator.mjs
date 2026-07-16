const schemaMetadataKeys = new Set([
	"$defs",
	"$schema",
	"description",
	"examples",
	"title",
]);

/** @param {any} schema @returns {any} */
export const typescriptSchema = (schema) => {
	if (!schema || typeof schema !== "object" || Array.isArray(schema))
		return schema;
	const output = { ...schema };
	for (const keyword of ["properties", "$defs"]) {
		if (!schema[keyword] || typeof schema[keyword] !== "object") continue;
		output[keyword] = Object.fromEntries(
			Object.entries(schema[keyword]).map(([name, value]) => [
				name,
				typescriptSchema(value),
			]),
		);
	}
	for (const keyword of ["oneOf", "anyOf", "allOf"]) {
		if (Array.isArray(schema[keyword])) {
			output[keyword] = schema[keyword].map(typescriptSchema);
		}
	}
	if (schema.items && !Array.isArray(schema.items)) {
		output.items = typescriptSchema(schema.items);
	}
	if (
		schema.additionalProperties &&
		typeof schema.additionalProperties === "object"
	) {
		output.additionalProperties = typescriptSchema(schema.additionalProperties);
	}
	if (Object.keys(schema).every((key) => schemaMetadataKeys.has(key))) {
		output.tsType = "unknown";
	}
	return output;
};

/** @param {string} declarations @returns {string} */
export const exactOptionalProperties = (declarations) => {
	const lines = declarations.split("\n");
	for (let index = 0; index < lines.length; index += 1) {
		const line = lines[index];
		if (line === undefined) continue;
		const property = /^(\s*)[A-Za-z_$][A-Za-z0-9_$]*\?:\s/u.exec(line);
		if (!property) continue;
		const indentation = property[1] ?? "";
		let end = index;
		while (true) {
			const candidate = lines[end];
			if (candidate === undefined) {
				throw new Error(
					`generated optional property has no terminator: ${line}; update the Rust schema or extend scripts/typescript-generator.mjs for this declaration shape, then rerun just sdk-generate`,
				);
			}
			const atPropertyIndent =
				candidate.startsWith(indentation) &&
				!candidate.slice(indentation.length).startsWith(" ");
			if (atPropertyIndent && candidate.trimEnd().endsWith(";")) break;
			end += 1;
		}
		lines[end] = lines[end]?.replace(/;\s*$/u, " | undefined;") ?? "";
		index = end;
	}
	return lines.join("\n");
};
