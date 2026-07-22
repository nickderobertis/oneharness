const METADATA_KEYS = new Set([
	"$defs",
	"$schema",
	"description",
	"examples",
	"title",
]);

/**
 * @typedef {object} JsonSchemaObject
 * @property {string | string[]=} type
 * @property {string=} title
 * @property {string=} description
 * @property {string=} $schema
 * @property {unknown[]=} examples
 * @property {string=} format
 * @property {string=} $ref
 * @property {unknown=} const
 * @property {unknown[]=} enum
 * @property {JsonSchemaNode[]=} oneOf
 * @property {JsonSchemaNode[]=} anyOf
 * @property {JsonSchemaNode[]=} allOf
 * @property {JsonSchemaNode=} not
 * @property {Record<string, JsonSchemaNode>=} properties
 * @property {JsonSchemaNode=} propertyNames
 * @property {string[]=} required
 * @property {JsonSchemaNode | JsonSchemaNode[]=} items
 * @property {boolean | JsonSchemaNode=} additionalProperties
 * @property {Record<string, JsonSchemaNode>=} $defs
 * @property {number=} minimum
 * @property {number=} maximum
 * @property {number=} exclusiveMinimum
 * @property {number=} exclusiveMaximum
 * @property {number=} multipleOf
 * @property {number=} minLength
 * @property {number=} maxLength
 * @property {string=} pattern
 * @property {number=} minItems
 * @property {number=} maxItems
 * @property {boolean=} uniqueItems
 * @property {number=} minProperties
 * @property {number=} maxProperties
 */

/** @typedef {boolean | JsonSchemaObject} JsonSchemaNode */

export const SDK_SCHEMA_ROOTS = Object.freeze([
	{ key: "run_options", type: "RunOptions", module: "options" },
	{ key: "history_lookup", type: "HistoryLookup", module: "history-lookup" },
	{
		key: "history_list_options",
		type: "HistoryListOptions",
		module: "history-list-options",
	},
	{
		key: "history_watch_options",
		type: "HistoryWatchOptions",
		module: "history-watch-options",
	},
	{ key: "run_report", type: "RunReport", module: "contracts" },
	{
		key: "run_stream_envelope",
		type: "RunStreamEnvelope",
		module: "run-stream-envelope",
	},
	{ key: "history_record", type: "HistoryRecord", module: "history" },
	{ key: "history_line", type: "HistoryLine", module: "history-line" },
	{
		key: "history_stream_envelope",
		type: "HistoryStreamEnvelope",
		module: "history-stream-envelope",
	},
	{
		key: "history_records",
		type: "HistoryRecords",
		module: "history-records",
		definitions: false,
	},
	{ key: "history_list", type: "HistoryList", module: "history-list" },
	{ key: "list_report", type: "ListReport", module: "registry" },
	{ key: "detect_report", type: "DetectReport", module: "detection" },
]);

export const SDK_SCHEMA_ALIASES = Object.freeze({
	BatchStrategy: 'BatchReport["strategy"]',
	ModeHeadless: 'ModeInfo["headless"]',
	SessionPhase: 'SessionReport["phase"]',
});

/** @param {unknown} value @returns {string} */
function literal(value) {
	return `z.literal(${JSON.stringify(value)})`;
}

/** @param {string[]} expressions @returns {string} */
function union(expressions) {
	if (expressions.length === 0) return "z.never()";
	if (expressions.length === 1) return expressions[0] ?? "z.never()";
	return `z.union([${expressions.join(", ")}])`;
}

/**
 * @param {JsonSchemaObject} schema
 * @param {Set<string>} supported
 * @param {string} path
 */
function assertSupported(schema, supported, path) {
	for (const key of Object.keys(schema)) {
		if (!METADATA_KEYS.has(key) && !supported.has(key)) {
			throw new Error(
				`unsupported JSON Schema keyword ${path}.${key}; update the Rust schema to avoid it or extend scripts/zod-generator.mjs to enforce it, then rerun just sdk-generate`,
			);
		}
	}
}

/** @param {string} reference @param {string} path @returns {string} */
function referenceName(reference, path) {
	const prefix = "#/$defs/";
	if (!reference.startsWith(prefix)) {
		throw new Error(`unsupported non-local JSON Schema reference at ${path}`);
	}
	const name = reference
		.slice(prefix.length)
		.replaceAll("~1", "/")
		.replaceAll("~0", "~");
	if (!/^[A-Za-z_$][A-Za-z0-9_$]*$/u.test(name)) {
		throw new Error(
			`unsupported JSON Schema definition name ${name} at ${path}`,
		);
	}
	return name;
}

/**
 * @param {JsonSchemaObject} schema
 * @param {string} path
 * @param {boolean} integer
 * @returns {string}
 */
function numberExpression(schema, path, integer) {
	assertSupported(
		schema,
		new Set([
			"type",
			"format",
			"minimum",
			"maximum",
			"exclusiveMinimum",
			"exclusiveMaximum",
			"multipleOf",
		]),
		path,
	);
	const integerFormats = new Set([
		"int32",
		"uint",
		"uint32",
		"uint64",
		"uint128",
	]);
	if (
		schema.format &&
		schema.format !== "double" &&
		!integerFormats.has(schema.format)
	) {
		throw new Error(
			`unsupported JSON Schema number format ${schema.format} at ${path}`,
		);
	}
	let expression =
		integer ||
		(schema.format !== undefined && integerFormats.has(schema.format))
			? "z.int()"
			: "z.number()";
	if (schema.minimum !== undefined)
		expression += `.gte(${JSON.stringify(schema.minimum)})`;
	if (schema.maximum !== undefined)
		expression += `.lte(${JSON.stringify(schema.maximum)})`;
	if (schema.exclusiveMinimum !== undefined) {
		expression += `.gt(${JSON.stringify(schema.exclusiveMinimum)})`;
	}
	if (schema.exclusiveMaximum !== undefined) {
		expression += `.lt(${JSON.stringify(schema.exclusiveMaximum)})`;
	}
	if (schema.multipleOf !== undefined) {
		expression += `.multipleOf(${JSON.stringify(schema.multipleOf)})`;
	}
	return expression;
}

/**
 * Read the single forbidden pattern a string schema's `not` may carry.
 *
 * @param {JsonSchemaNode} forbidden
 * @param {string} path
 * @returns {string}
 */
function forbiddenPattern(forbidden, path) {
	if (
		!forbidden ||
		typeof forbidden !== "object" ||
		Object.keys(forbidden).length !== 1 ||
		typeof forbidden.pattern !== "string"
	) {
		throw new Error(
			`unsupported JSON Schema \`not\` at ${path}; only a lone forbidden \`pattern\` is enforced — extend scripts/zod-generator.mjs to enforce this shape, then rerun just sdk-generate`,
		);
	}
	return forbidden.pattern;
}

/** @param {JsonSchemaObject} schema @param {string} path @returns {string} */
function stringExpression(schema, path) {
	assertSupported(
		schema,
		new Set(["type", "minLength", "maxLength", "pattern", "not", "format"]),
		path,
	);
	if (schema.format) {
		throw new Error(
			`unsupported JSON Schema string format ${schema.format} at ${path}`,
		);
	}
	let expression = "z.string()";
	if (schema.minLength !== undefined) expression += `.min(${schema.minLength})`;
	if (schema.pattern !== undefined)
		expression += `.regex(new RegExp(${JSON.stringify(schema.pattern)}, "u"))`;
	// JSON Schema measures a string's length in Unicode code points, while Zod's
	// `.max()` measures JavaScript's UTF-16 code units — so an astral character
	// would count twice and this validator would reject a value Rust and the
	// Python SDK both accept. Spread to count code points instead.
	if (schema.maxLength !== undefined) {
		expression += `.refine((value) => [...value].length <= ${schema.maxLength}, { message: ${JSON.stringify(
			`Too long: expected at most ${schema.maxLength} characters`,
		)} })`;
	}
	if (schema.not !== undefined) {
		const forbidden = forbiddenPattern(schema.not, `${path}.not`);
		expression += `.refine((value) => !new RegExp(${JSON.stringify(forbidden)}, "u").test(value), { message: ${JSON.stringify(
			`Invalid string: must not contain ${forbidden}`,
		)} })`;
	}
	return expression;
}

/** @param {JsonSchemaObject} schema @param {string} path @returns {string} */
function arrayExpression(schema, path) {
	assertSupported(
		schema,
		new Set(["type", "items", "minItems", "maxItems", "uniqueItems"]),
		path,
	);
	if (!schema.items || Array.isArray(schema.items)) {
		throw new Error(`array schema needs one items schema at ${path}`);
	}
	let expression = `z.array(${schemaExpression(schema.items, `${path}.items`)})`;
	if (schema.minItems !== undefined) expression += `.min(${schema.minItems})`;
	if (schema.maxItems !== undefined) expression += `.max(${schema.maxItems})`;
	if (schema.uniqueItems) {
		throw new Error(`unsupported JSON Schema keyword ${path}.uniqueItems`);
	}
	return expression;
}

/** @param {JsonSchemaObject} schema @param {string} path @returns {string} */
function objectExpression(schema, path) {
	assertSupported(
		schema,
		new Set([
			"type",
			"properties",
			"propertyNames",
			"required",
			"additionalProperties",
			"minProperties",
			"maxProperties",
		]),
		path,
	);
	if (
		schema.minProperties !== undefined ||
		schema.maxProperties !== undefined
	) {
		throw new Error(
			`unsupported JSON Schema object size constraint at ${path}`,
		);
	}
	const properties = schema.properties ?? {};
	const required = new Set(schema.required ?? []);
	for (const name of required) {
		if (!(name in properties))
			throw new Error(`required property ${path}.${name} has no schema`);
	}
	const fields = Object.keys(properties)
		.sort()
		.map((name) => {
			const property = properties[name];
			if (property === undefined) {
				throw new Error(`property ${path}.${name} has no schema`);
			}
			let expression = schemaExpression(property, `${path}.properties.${name}`);
			if (required.has(name)) {
				expression +=
					'.refine((value) => value !== undefined, { message: "Required" })';
			} else expression += ".optional()";
			return `${JSON.stringify(name)}: ${expression}`;
		});
	const shape = `{${fields.length === 0 ? "" : `\n\t\t${fields.join(",\n\t\t")},\n\t`}}`;
	if (schema.additionalProperties === false) return `z.strictObject(${shape})`;
	if (schema.additionalProperties && schema.additionalProperties !== true) {
		const keySchema = schema.propertyNames
			? schemaExpression(schema.propertyNames, `${path}.propertyNames`)
			: "z.string()";
		if (fields.length === 0) {
			return `z.record(${keySchema}, ${schemaExpression(schema.additionalProperties, `${path}.additionalProperties`)})`;
		}
		if (schema.propertyNames) {
			throw new Error(
				`propertyNames with declared properties is unsupported at ${path}`,
			);
		}
		return `z.object(${shape}).catchall(${schemaExpression(schema.additionalProperties, `${path}.additionalProperties`)})`;
	}
	if (schema.propertyNames) {
		throw new Error(`propertyNames requires additionalProperties at ${path}`);
	}
	// Rust output structs intentionally omit `additionalProperties: false`.
	// Preserve future fields instead of Zod's default stripping behavior.
	return `z.looseObject(${shape})`;
}

/** @param {JsonSchemaNode} schema @param {string} path @returns {string} */
function schemaExpression(schema, path) {
	if (schema === true) return "z.unknown()";
	if (schema === false) return "z.never()";
	if (!schema || typeof schema !== "object" || Array.isArray(schema)) {
		throw new Error(`invalid JSON Schema node at ${path}`);
	}
	if (schema.$ref !== undefined) {
		assertSupported(schema, new Set(["$ref"]), path);
		return `z.lazy(() => ${referenceName(schema.$ref, path)}Schema)`;
	}
	if (schema.const !== undefined) {
		assertSupported(schema, new Set(["const", "type"]), path);
		return literal(schema.const);
	}
	if (schema.enum !== undefined) {
		assertSupported(schema, new Set(["enum", "type"]), path);
		return union(schema.enum.map(literal));
	}
	if (schema.oneOf !== undefined || schema.anyOf !== undefined) {
		const keyword = schema.oneOf ? "oneOf" : "anyOf";
		assertSupported(schema, new Set([keyword]), path);
		const members = schema.oneOf ?? schema.anyOf ?? [];
		return union(
			members.map((member, index) =>
				schemaExpression(member, `${path}.${keyword}.${index}`),
			),
		);
	}
	if (schema.allOf !== undefined) {
		assertSupported(schema, new Set(["allOf"]), path);
		const expressions = schema.allOf.map((member, index) =>
			schemaExpression(member, `${path}.allOf.${index}`),
		);
		if (expressions.length === 0) return "z.unknown()";
		return expressions
			.slice(1)
			.reduce(
				(left, right) => `z.intersection(${left}, ${right})`,
				expressions[0] ?? "z.unknown()",
			);
	}
	if (Array.isArray(schema.type)) {
		/** @type {Record<string, Set<string>>} */
		const typeKeywords = {
			array: new Set(["items", "minItems", "maxItems", "uniqueItems"]),
			boolean: new Set(),
			integer: new Set([
				"format",
				"minimum",
				"maximum",
				"exclusiveMinimum",
				"exclusiveMaximum",
				"multipleOf",
			]),
			null: new Set(),
			number: new Set([
				"format",
				"minimum",
				"maximum",
				"exclusiveMinimum",
				"exclusiveMaximum",
				"multipleOf",
			]),
			object: new Set([
				"properties",
				"required",
				"additionalProperties",
				"minProperties",
				"maxProperties",
			]),
			string: new Set(["minLength", "maxLength", "pattern", "not", "format"]),
		};
		// Each member keeps only the keywords that constrain its own type, so a
		// keyword none of these types claims would be filtered away silently
		// rather than enforced. Reject it here instead.
		assertSupported(
			schema,
			new Set([
				"type",
				...schema.type.flatMap((type) => [...(typeKeywords[type] ?? [])]),
			]),
			path,
		);
		return union(
			schema.type.map((type) => {
				const allowed = typeKeywords[type];
				if (!allowed)
					throw new Error(`unsupported JSON Schema type ${type} at ${path}`);
				/** @type {JsonSchemaObject} */
				const member = Object.fromEntries(
					Object.entries(schema).filter(
						([key]) =>
							key === "type" || METADATA_KEYS.has(key) || allowed.has(key),
					),
				);
				member.type = type;
				return schemaExpression(member, path);
			}),
		);
	}
	switch (schema.type) {
		case "object":
			return objectExpression(schema, path);
		case "array":
			return arrayExpression(schema, path);
		case "string":
			return stringExpression(schema, path);
		case "integer":
			return numberExpression(schema, path, true);
		case "number":
			return numberExpression(schema, path, false);
		case "boolean":
			assertSupported(schema, new Set(["type"]), path);
			return "z.boolean()";
		case "null":
			assertSupported(schema, new Set(["type"]), path);
			return "z.null()";
		case undefined:
			assertSupported(schema, new Set(), path);
			return "z.unknown()";
		default:
			throw new Error(`unsupported JSON Schema type ${schema.type} at ${path}`);
	}
}

/** @param {unknown} schema @returns {unknown} */
function comparableSchema(schema) {
	if (Array.isArray(schema)) return schema.map(comparableSchema);
	if (!schema || typeof schema !== "object") return schema;
	return Object.fromEntries(
		Object.entries(schema)
			.filter(([key]) => !METADATA_KEYS.has(key))
			.map(([key, value]) => [key, comparableSchema(value)]),
	);
}

/** @param {unknown} left @param {unknown} right @returns {boolean} */
function sameSchema(left, right) {
	return (
		JSON.stringify(comparableSchema(left)) ===
		JSON.stringify(comparableSchema(right))
	);
}

/**
 * Generate one deterministic Zod module from Rust's JSON Schema bundle.
 *
 * @param {Record<string, JsonSchemaObject>} bundle
 * @param {ReadonlyArray<{ key: string, type: string, module: string, definitions?: boolean }>} roots
 * @param {Readonly<Record<string, string>>} aliases
 */
export function generateZodModule(bundle, roots, aliases = {}) {
	/** @type {Map<string, { schema: JsonSchemaObject, module: string, path: string }>} */
	const named = new Map();
	/**
	 * @param {string} name
	 * @param {JsonSchemaObject} schema
	 * @param {string} module
	 * @param {string} path
	 */
	const add = (name, schema, module, path) => {
		const existing = named.get(name);
		if (existing) {
			if (!sameSchema(existing.schema, schema)) {
				throw new Error(
					`conflicting Rust schemas named ${name} at ${existing.path} and ${path}`,
				);
			}
			return;
		}
		named.set(name, { schema, module, path });
	};

	for (const root of roots) {
		const schema = bundle[root.key];
		if (!schema) throw new Error(`Rust schema bundle is missing ${root.key}`);
		add(root.type, schema, root.module, root.key);
	}
	for (const root of roots) {
		if (root.definitions === false) continue;
		const document = bundle[root.key];
		if (!document) throw new Error(`Rust schema bundle is missing ${root.key}`);
		const definitions = document.$defs ?? {};
		for (const name of Object.keys(definitions).sort()) {
			const definition = definitions[name];
			if (typeof definition !== "object") {
				throw new Error(
					`Rust schema definition ${root.key}.${name} is not an object`,
				);
			}
			add(name, definition, root.module, `${root.key}.$defs.${name}`);
		}
	}

	/** @type {Map<string, string[]>} */
	const imports = new Map();
	for (const [name, value] of named) {
		if (aliases[name]) continue;
		const names = imports.get(value.module) ?? [];
		names.push(name);
		imports.set(value.module, names);
	}
	const lines = [
		"/* Generated from oneharness Rust JSON Schemas. Do not edit. */",
		"",
		'import { z } from "zod";',
	];
	for (const module of [...imports.keys()].sort()) {
		const names = [...(imports.get(module) ?? [])].sort();
		lines.push(`import type { ${names.join(", ")} } from "./${module}.js";`);
	}
	lines.push("");
	for (const name of Object.keys(aliases).sort()) {
		lines.push(`export type ${name} = ${aliases[name]};`);
	}
	if (Object.keys(aliases).length > 0) lines.push("");
	for (const name of [...named.keys()].sort()) {
		const value = named.get(name);
		if (!value) throw new Error(`missing collected schema ${name}`);
		lines.push(
			`export const ${name}Schema: z.ZodType<${name}> = ${schemaExpression(value.schema, value.path)};`,
			"",
		);
	}
	return `${lines.join("\n").trimEnd()}\n`;
}
