pub const LANGUAGE_NAME: &str = "rhai";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParameterDoc {
    pub name: &'static str,
    pub kind: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FunctionDoc {
    pub name: &'static str,
    pub parameters: &'static [ParameterDoc],
    pub returns: Option<&'static str>,
    pub doc: &'static str,
}

impl FunctionDoc {
    pub fn signature(&self) -> String {
        let parameters = self
            .parameters
            .iter()
            .map(|parameter| format!("{}: {}", parameter.name, parameter.kind))
            .collect::<Vec<_>>()
            .join(", ");
        match self.returns {
            Some(kind) => format!("{}({parameters}) -> {kind}", self.name),
            None => format!("{}({parameters})", self.name),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VariableDoc {
    pub name: &'static str,
    pub kind: &'static str,
    pub doc: &'static str,
}

const fn param(name: &'static str, kind: &'static str) -> ParameterDoc {
    ParameterDoc { name, kind }
}

const fn function(
    name: &'static str,
    parameters: &'static [ParameterDoc],
    returns: Option<&'static str>,
    doc: &'static str,
) -> FunctionDoc {
    FunctionDoc {
        name,
        parameters,
        returns,
        doc,
    }
}

const fn variable(name: &'static str, kind: &'static str, doc: &'static str) -> VariableDoc {
    VariableDoc { name, kind, doc }
}

pub const KEYWORDS: &[&str] = &[
    "let", "const", "if", "else", "switch", "while", "loop", "do", "until", "for", "in", "fn",
    "return", "break", "continue", "throw", "try", "catch", "true", "false",
];

pub const FUNCTIONS: &[FunctionDoc] = &[
    function(
        "json_parse",
        &[param("text", "string")],
        Some("any"),
        "Parses JSON text into maps, arrays and scalars; fails on invalid or oversized JSON.",
    ),
    function(
        "json_stringify",
        &[param("value", "any")],
        Some("string"),
        "Writes a value as compact JSON text.",
    ),
    function(
        "base64_encode",
        &[param("text", "string")],
        Some("string"),
        "Encodes text as standard base64.",
    ),
    function(
        "base64_decode",
        &[param("text", "string")],
        Some("string"),
        "Decodes standard base64 into UTF-8 text; fails on anything else.",
    ),
    function(
        "hex",
        &[param("text", "string")],
        Some("string"),
        "Returns the UTF-8 bytes of the text as lowercase hex.",
    ),
    function(
        "sha256",
        &[param("text", "string")],
        Some("string"),
        "Returns the SHA-256 digest of the text as lowercase hex.",
    ),
    function(
        "hmac_sha256",
        &[param("key", "string"), param("text", "string")],
        Some("string"),
        "Returns the HMAC-SHA256 of the text under the key as lowercase hex.",
    ),
    function(
        "regex_match",
        &[param("pattern", "string"), param("text", "string")],
        Some("bool"),
        "True when the regular expression matches somewhere in the text.",
    ),
    function(
        "regex_captures",
        &[param("pattern", "string"), param("text", "string")],
        Some("array"),
        "Returns the capture groups of the first match, group 0 first and () for a group that did not take part; empty when nothing matches.",
    ),
    function("uuid", &[], Some("string"), "Returns a random UUID."),
    function(
        "random_int",
        &[param("low", "int"), param("high", "int")],
        Some("int"),
        "Returns a random integer from low to high, both included.",
    ),
    function(
        "now",
        &[],
        Some("string"),
        "Returns the capture time as an RFC 3339 timestamp.",
    ),
    function(
        "replace",
        &[
            param("text", "string"),
            param("find", "string"),
            param("substitute", "string"),
        ],
        None,
        "Replaces every occurrence of find in the text variable itself; call it as text.replace(find, substitute).",
    ),
    function(
        "pad",
        &[
            param("text", "string"),
            param("length", "int"),
            param("padding", "string"),
        ],
        None,
        "Pads the text variable itself up to length characters with padding; call it as text.pad(length, padding).",
    ),
    function(
        "len",
        &[param("value", "any")],
        Some("int"),
        "Returns the length of a string, array or map.",
    ),
    function(
        "contains",
        &[param("value", "any"), param("item", "any")],
        Some("bool"),
        "True when a string holds the text, an array the item or a map the key.",
    ),
    function(
        "starts_with",
        &[param("text", "string"), param("prefix", "string")],
        Some("bool"),
        "True when the text starts with the prefix.",
    ),
    function(
        "ends_with",
        &[param("text", "string"), param("suffix", "string")],
        Some("bool"),
        "True when the text ends with the suffix.",
    ),
    function(
        "trim",
        &[param("text", "string")],
        None,
        "Removes whitespace from both ends of the text variable itself.",
    ),
    function(
        "to_upper",
        &[param("text", "string")],
        Some("string"),
        "Returns the text in upper case.",
    ),
    function(
        "to_lower",
        &[param("text", "string")],
        Some("string"),
        "Returns the text in lower case.",
    ),
    function(
        "split",
        &[param("text", "string"), param("separator", "string")],
        Some("array"),
        "Splits the text at every separator.",
    ),
    function(
        "sub_string",
        &[
            param("text", "string"),
            param("start", "int"),
            param("length", "int"),
        ],
        Some("string"),
        "Returns length characters of the text from start.",
    ),
    function(
        "to_string",
        &[param("value", "any")],
        Some("string"),
        "Converts a value to text.",
    ),
    function(
        "parse_int",
        &[param("text", "string")],
        Some("int"),
        "Parses a whole number; fails when the text is not one.",
    ),
    function(
        "parse_float",
        &[param("text", "string")],
        Some("float"),
        "Parses a decimal number; fails when the text is not one.",
    ),
    function(
        "keys",
        &[param("map", "map")],
        Some("array"),
        "Returns the keys of a map.",
    ),
    function(
        "values",
        &[param("map", "map")],
        Some("array"),
        "Returns the values of a map.",
    ),
    function(
        "push",
        &[param("list", "array"), param("item", "any")],
        None,
        "Appends an item to the array variable itself.",
    ),
    function(
        "pop",
        &[param("list", "array")],
        Some("any"),
        "Removes and returns the last item of the array variable itself.",
    ),
    function(
        "print",
        &[param("value", "any")],
        None,
        "Writes a line to the run log, 4 KiB in total.",
    ),
];

pub const VARIABLES: &[VariableDoc] = &[
    variable("request", "map", "The captured request. Read-only."),
    variable("request.method", "string", "The HTTP method, upper case."),
    variable(
        "request.path",
        "string",
        "The request path after the endpoint id.",
    ),
    variable(
        "request.segments",
        "array",
        "The path segments after the endpoint id.",
    ),
    variable(
        "request.query",
        "map",
        "Query parameters by name; the first value of each.",
    ),
    variable(
        "request.headers",
        "map",
        "Request headers by lower-case name; the first value of each.",
    ),
    variable(
        "request.form",
        "map",
        "Form fields by name; the first value of each.",
    ),
    variable(
        "request.body",
        "string",
        "The body as text, the first 64 KiB; parse JSON with json_parse(request.body).",
    ),
    variable("request.ip", "string", "The sender's address."),
    variable(
        "request.content_type",
        "string",
        "The request's content type.",
    ),
    variable(
        "request.kind",
        "string",
        "How the capture arrived: http, email, dns or schedule.",
    ),
    variable(
        "request.meta",
        "map",
        "Extra facts of the capture by name, such as the sender of an email.",
    ),
    variable(
        "request.signature",
        "string",
        "The result of signature verification: valid, invalid or missing.",
    ),
    variable("request.capture", "map", "The capture."),
    variable("request.capture.id", "string", "The capture id."),
    variable(
        "request.capture.seq",
        "int",
        "The capture's sequence number.",
    ),
    variable(
        "request.capture.received_at",
        "string",
        "When the capture arrived, as an RFC 3339 timestamp.",
    ),
    variable("request.endpoint", "map", "The endpoint."),
    variable("request.endpoint.id", "string", "The endpoint id."),
    variable(
        "request.endpoint.url",
        "string",
        "The endpoint's capture URL.",
    ),
    variable(
        "request.endpoint.title",
        "string",
        "The endpoint's title, empty when it has none.",
    ),
    variable(
        "request.store",
        "map",
        "The endpoint's persistent variables by name.",
    ),
    variable(
        "request.counter",
        "map",
        "The endpoint's persistent counters by name.",
    ),
    variable(
        "vars",
        "map",
        "Run variables. Changes leave the script only through the returned map.",
    ),
];
