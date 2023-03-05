#![doc = include_str!("../README.md")]

use serde::{Deserialize, Serialize};

#[derive(Debug)]
pub enum Error {
    PreProcessing(String),
    Metadata(String),
}

impl std::error::Error for Error {}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ERROR: {}",
            match self {
                Error::PreProcessing(err) => format!("pre-processing error: {err}"),
                Error::Metadata(err) =>
                    format!("failed to parse function or module metadata: {err}"),
            }
        )
    }
}

#[derive(Debug)]
/// Rhai module documentation in markdown format.
pub struct ModuleDocumentation {
    /// Name of the module.
    pub name: String,
    /// Sub modules.
    pub sub_modules: Vec<ModuleDocumentation>,
    /// Raw text documentation in markdown.
    pub documentation: String,
}

/// Intermediatory representation of the documentation.
#[derive(Serialize, Deserialize, Debug, Clone)]
struct ModuleMetadata {
    /// Optional documentation for the module.
    doc: Option<String>,
    /// Functions metadata, if any.
    functions: Option<Vec<FunctionMetadata>>,
    /// Sub-modules, if any, stored as raw json values.
    modules: Option<serde_json::Map<String, serde_json::Value>>,
}

impl ModuleMetadata {
    /// Format the module doc comments to make them
    /// readable markdown.
    pub fn fmt_doc_comments(&self) -> Option<String> {
        self.doc
            .clone()
            .map(|dc| remove_test_code(&fmt_doc_comments(dc)))
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
struct FunctionMetadata {
    pub access: String,
    pub base_hash: u128,
    pub full_hash: u128,
    pub name: String,
    pub namespace: String,
    pub num_params: usize,
    pub params: Option<Vec<std::collections::HashMap<String, String>>>,
    pub signature: String,
    pub return_type: Option<String>,
    pub doc_comments: Option<Vec<String>>,
}

impl FunctionMetadata {
    /// Format the function doc comments to make them
    /// readable markdown.
    pub fn fmt_doc_comments(&self) -> Option<String> {
        self.doc_comments
            .clone()
            .map(|dc| remove_test_code(&fmt_doc_comments(remove_extra_tokens(dc))))
    }
}

/// Remove doc comments identifiers.
fn fmt_doc_comments(dc: String) -> String {
    dc.replace("/// ", "")
        .replace("///", "")
        .replace("/**", "")
        .replace("**/", "")
        .replace("**/", "")
}

/// Remove crate specific comments, like `rhai-autodocs:index`.
fn remove_extra_tokens(dc: Vec<String>) -> String {
    dc.into_iter()
        .map(|s| {
            s.lines()
                .filter(|l| !l.contains(RHAI_FUNCTION_INDEX_PATTERN))
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// NOTE: mdbook handles this automatically, but other
///       markdown processors might not.
fn remove_test_code(doc_comments: &str) -> String {
    let mut formatted = vec![];
    let mut in_code_block = false;
    for line in doc_comments.lines() {
        if line.trim() == "```" {
            in_code_block = !in_code_block;
            formatted.push(line);
            continue;
        }

        if !(in_code_block && line.starts_with('#') && !line.starts_with("#{")) {
            formatted.push(line);
        }
    }

    formatted.join("\n")
}

#[derive(Default)]
/// Select in which order each functions will be displayed.
pub enum FunctionOrder {
    /// Display functions by alphabetical order.
    #[default]
    Alphabetical,
    /// Display functions by index using a pre-processing comment with the `# rhai-autodocs:index:<number>` syntax.
    /// The `# rhai-autodocs:index:<number>` line will be removed in the final generated markdown.
    ///
    /// # Example
    ///
    /// ```ignore
    /// /// Function that will appear first in docs.
    /// ///
    /// /// # rhai-autodocs:index:1
    /// #[rhai_fn(global)]
    /// pub fn my_function1() {}
    ///
    /// /// Function that will appear second in docs.
    /// ///
    /// /// # rhai-autodocs:index:2
    /// #[rhai_fn(global)]
    /// pub fn my_function2() {}
    /// ```
    ByIndex,
}

const RHAI_FUNCTION_INDEX_PATTERN: &str = "# rhai-autodocs:index:";

impl FunctionOrder {
    fn order_function_groups<'a>(
        &'_ self,
        mut function_groups: Vec<(String, Vec<&'a FunctionMetadata>)>,
    ) -> Result<Vec<(String, Vec<&'a FunctionMetadata>)>, Error> {
        match self {
            FunctionOrder::Alphabetical => {
                function_groups.sort_by(|(a, _), (b, _)| a.cmp(b));

                Ok(function_groups)
            }
            FunctionOrder::ByIndex => {
                let mut ordered = function_groups.clone();

                'groups: for (function, polymorphisms) in function_groups {
                    for metadata in polymorphisms
                        .iter()
                        .filter_map(|item| item.doc_comments.as_ref())
                    {
                        if let Some((_, index)) = metadata
                            .iter()
                            .find_map(|line| line.rsplit_once(RHAI_FUNCTION_INDEX_PATTERN))
                        {
                            let index = index
                                .parse::<usize>()
                                .map_err(|err| Error::PreProcessing(err.to_string()))?;

                            ordered[index - 1] = (function, polymorphisms);
                            continue 'groups;
                        }
                    }

                    return Err(Error::PreProcessing(format!(
                        "missing ord metadata in function {function}"
                    )));
                }

                Ok(ordered)
            }
        }
    }
}

#[derive(Default)]
/// Options to configure documentation generation.
pub struct Options {
    order: FunctionOrder,
    include_standard_packages: bool,
}

/// Create new options used to configure docs generation.
pub fn options() -> Options {
    Options::default()
}

impl Options {
    /// Include the standard package functions and modules documentation
    /// in the generated documentation markdown.
    pub fn include_standard_packages(mut self, include_standard_packages: bool) -> Self {
        self.include_standard_packages = include_standard_packages;

        self
    }

    /// Order functions in a specific way.
    /// See [`FunctionOrder`] for more details.
    pub fn order_with(mut self, order: FunctionOrder) -> Self {
        self.order = order;

        self
    }

    /// Generate documentation based on an engine instance.
    /// Make sure all the functions, operators, plugins, etc. are registered inside this instance.
    ///
    /// # Result
    /// * A vector of documented modules.
    ///
    /// # Errors
    /// * Failed to generate function metadata as json.
    /// * Failed to parse module metadata.
    pub fn generate(self, engine: &rhai::Engine) -> Result<ModuleDocumentation, Error> {
        generate_documentation(engine, self)
    }
}

/// Generate documentation based on an engine instance.
/// Make sure all the functions, operators, plugins, etc. are registered inside this instance.
///
/// # Result
/// * A vector of documented modules.
///
/// # Errors
/// * Failed to generate function metadata as json.
/// * Failed to parse module metadata.
fn generate_documentation(
    engine: &rhai::Engine,
    options: Options,
) -> Result<ModuleDocumentation, Error> {
    let json_fns = engine
        .gen_fn_metadata_to_json(options.include_standard_packages)
        .map_err(|error| Error::Metadata(error.to_string()))?;

    let metadata = serde_json::from_str::<ModuleMetadata>(&json_fns)
        .map_err(|error| Error::Metadata(error.to_string()))?;

    generate_module_documentation(engine, &options, "global", metadata)
}

fn generate_module_documentation(
    engine: &rhai::Engine,
    options: &Options,
    namespace: &str,
    metadata: ModuleMetadata,
) -> Result<ModuleDocumentation, Error> {
    let mut md = ModuleDocumentation {
        name: namespace.to_owned(),
        sub_modules: vec![],
        documentation: format!(
            "# {namespace}\n\n{}",
            metadata
                .fmt_doc_comments()
                .map_or_else(String::default, |doc| format!("{doc}\n\n"))
        ),
    };

    if let Some(functions) = metadata.functions {
        let mut function_groups =
            std::collections::HashMap::<String, Vec<&FunctionMetadata>>::default();

        // Rhai function can be polymorphes, so we group them by name.
        functions.iter().for_each(|metadata| {
            match function_groups.get_mut(&metadata.name) {
                Some(polymorphisms) => polymorphisms.push(metadata),
                None => {
                    function_groups.insert(metadata.name.clone(), vec![metadata]);
                }
            };
        });

        let function_groups = function_groups
            .into_iter()
            .map(|(name, polymorphisms)| (name, polymorphisms))
            .collect::<Vec<_>>();

        let fn_groups = options.order.order_function_groups(function_groups)?;

        // Generate a clean documentation for each functions.
        // Functions that share the same name will keep only
        // one documentation, the others will be dropped.
        //
        // This means that:
        // ```rust
        // /// doc 1
        // fn my_func(a: int)`
        // ```
        // and
        // ```rust
        // /// doc 2
        // fn my_func(a: int, b: int)`
        // ```
        // will be written as the following:
        // ```rust
        // /// doc 1
        // fn my_func(a: int);
        // fn my_func(a: int, b: int);
        // ```
        for (name, polymorphisms) in fn_groups {
            if let Some(fn_doc) = generate_function_documentation(
                engine,
                &name.replace("get$", "").replace("set$", ""),
                &polymorphisms[..],
            ) {
                md.documentation += &fn_doc;
            }
        }
    }

    // Generate documentation for each submodule. (if any)
    if let Some(sub_modules) = metadata.modules {
        for (sub_module, value) in sub_modules {
            md.sub_modules.push(generate_module_documentation(
                engine,
                options,
                &format!("{namespace}::{sub_module}"),
                serde_json::from_value::<ModuleMetadata>(value)
                    .map_err(|error| Error::Metadata(error.to_string()))?,
            )?);
        }
    }

    Ok(md)
}

/// Generate markdown/html documentation for a function.
/// TODO: Add other word processors.
fn generate_function_documentation(
    engine: &rhai::Engine,
    name: &str,
    polymorphisms: &[&FunctionMetadata],
) -> Option<String> {
    let metadata = polymorphisms.first().expect("will never be empty");
    let root_definition = generate_function_definition(engine, metadata);

    // Anonymous functions are ignored.
    if !name.starts_with("anon$") {
        Some(format!(
            r#"
<div markdown="span" style='box-shadow: 0 4px 8px 0 rgba(0,0,0,0.2); padding: 15px; border-radius: 5px;'>

<h2 class="func-name"> <code>{}</code> {} </h2>

```rust,ignore
{}
```
{}
</div>
</br>
"#,
            // Add a specific prefix for the function type documented.
            if root_definition.starts_with("op") {
                "op"
            } else if root_definition.starts_with("fn get ") {
                "get"
            } else if root_definition.starts_with("fn set ") {
                "set"
            } else {
                "fn"
            },
            name,
            polymorphisms
                .iter()
                .map(|metadata| generate_function_definition(engine, metadata))
                .collect::<Vec<_>>()
                .join("\n"),
            &metadata
                .fmt_doc_comments()
                .map(|doc| format!(
                    r#"
<details>
<summary markdown="span"> details </summary>

{doc}
</details>
"#
                ))
                .unwrap_or_default()
        ))
    } else {
        None
    }
}

fn is_operator(name: &str) -> bool {
    ["==", "!=", ">", ">=", "<", "<=", "in"]
        .into_iter()
        .any(|op| op == name)
}

/// Generate a pseudo-Rust definition of a rhai function.
/// e.g. `fn my_func(a: int) -> ()`
fn generate_function_definition(engine: &rhai::Engine, metadata: &FunctionMetadata) -> String {
    // Add the operator / function prefix.
    let mut definition = if is_operator(&metadata.name) {
        String::from("op ")
    } else {
        String::from("fn ")
    };

    // Add getter and setter prefix + the name of the function.
    if let Some(name) = metadata.name.strip_prefix("get$") {
        definition += &format!("get {name}(");
    } else if let Some(name) = metadata.name.strip_prefix("set$") {
        definition += &format!("set {name}(");
    } else {
        definition += &format!("{}(", metadata.name);
    }

    let mut first = true;

    // Add params with their types.
    for i in 0..metadata.num_params {
        if !first {
            definition += ", ";
        }
        first = false;

        let (param_name, param_type) = metadata
            .params
            .as_ref()
            .expect("metadata.num_params does not match the number of parameters")
            .get(i)
            .map_or(("_", "?".into()), |s| {
                (
                    s.get("name").map(|s| s.as_str()).unwrap_or("_"),
                    s.get("type").map_or(std::borrow::Cow::Borrowed("?"), |ty| {
                        def_type_name(ty, engine)
                    }),
                )
            });

        definition += &format!("{param_name}: {param_type}");
    }

    // Add an eventual return type.
    if let Some(return_type) = &metadata.return_type {
        definition + format!(") -> {}", def_type_name(return_type, engine)).as_str()
    } else {
        definition + ")"
    }
}

/// This is the code a private function in the rhai crate. It is used to map
/// "Rust" types to a more user readable format. Here is the documentation of the
/// original function:
///
/// """
/// We have to transform some of the types.
///
/// This is highly inefficient and is currently based on trial and error with the core packages.
///
/// It tries to flatten types, removing `&` and `&mut`, and paths, while keeping generics.
///
/// Associated generic types are also rewritten into regular generic type parameters.
/// """
fn def_type_name<'a>(ty: &'a str, _: &'a rhai::Engine) -> std::borrow::Cow<'a, str> {
    let ty = ty.strip_prefix("&mut").unwrap_or(ty).trim();
    let ty = remove_result(ty);
    // Removes namespaces for the type.
    let ty = ty.split("::").last().unwrap();

    let ty = ty
        .replace("Iterator<Item=", "Iterator<")
        .replace("Dynamic", "?")
        .replace("INT", "int")
        .replace(std::any::type_name::<rhai::INT>(), "int")
        .replace("FLOAT", "float")
        .replace("&str", "String")
        .replace("ImmutableString", "String");

    let ty = ty.replace(std::any::type_name::<rhai::FLOAT>(), "float");
    let ty = ty.replace(std::any::type_name::<rhai::Array>(), "Array");
    let ty = ty.replace(std::any::type_name::<rhai::Blob>(), "Blob");
    let ty = ty.replace(std::any::type_name::<rhai::Map>(), "Map");
    let ty = ty.replace(std::any::type_name::<rhai::Instant>(), "Instant");
    let ty = ty.replace(std::any::type_name::<rhai::FnPtr>(), "FnPtr");

    ty.into()
}

/// Remove the result wrapper for a return type since it can be confusing in the documentation
/// NOTE: should we replace the wrapper by a '!' character or a tag on the function definition ?
fn remove_result(ty: &str) -> &str {
    if let Some(ty) = ty.strip_prefix("Result<") {
        ty.strip_suffix(",Box<EvalAltResult>>")
            .or_else(|| ty.strip_suffix(",Box<rhai::EvalAltResult>>"))
            .or_else(|| ty.strip_suffix(", Box<EvalAltResult>>"))
            .or_else(|| ty.strip_suffix(", Box<rhai::EvalAltResult>>"))
    } else if let Some(ty) = ty.strip_prefix("EngineResult<") {
        ty.strip_suffix('>')
    } else if let Some(ty) = ty
        .strip_prefix("RhaiResultOf<")
        .or_else(|| ty.strip_prefix("rhai::RhaiResultOf<"))
    {
        ty.strip_suffix('>')
    } else {
        None
    }
    .map_or(ty, str::trim)
}

#[cfg(test)]
pub mod test {
    use super::*;

    #[test]
    fn test_remove_result() {
        assert_eq!("Cache", remove_result("Result<Cache, Box<EvalAltResult>>"));
        assert_eq!("Cache", remove_result("Result<Cache,Box<EvalAltResult>>"));
        assert_eq!(
            "&mut Cache",
            remove_result("Result<&mut Cache, Box<EvalAltResult>>")
        );
        assert_eq!(
            "Cache",
            remove_result("Result<Cache, Box<rhai::EvalAltResult>>")
        );
        assert_eq!(
            "Cache",
            remove_result("Result<Cache,Box<rhai::EvalAltResult>>")
        );
        assert_eq!("Stuff", remove_result("EngineResult<Stuff>"));
        assert_eq!("Stuff", remove_result("RhaiResultOf<Stuff>"));
        assert_eq!("Stuff", remove_result("rhai::RhaiResultOf<Stuff>"));
    }

    use rhai::plugin::*;

    /// My own module.
    #[export_module]
    mod my_module {
        /// A function that prints to stdout.
        ///
        /// # rhai-autodocs:index:1
        #[rhai_fn(global)]
        pub fn hello_world() {
            println!("Hello, World!");
        }

        /// A function that adds two integers together.
        ///
        /// # rhai-autodocs:index:2
        #[rhai_fn(global)]
        pub fn add(a: rhai::INT, b: rhai::INT) -> rhai::INT {
            a + b
        }
    }

    #[test]
    fn test_order_by_index() {
        let mut engine = rhai::Engine::new();

        engine.register_static_module("my_module", exported_module!(my_module).into());

        // register custom functions and types ...
        let docs = crate::options()
            .include_standard_packages(false)
            .order_with(crate::FunctionOrder::ByIndex)
            .generate(&engine)
            .expect("failed to generate documentation");

        assert_eq!(docs.name, "global");
        assert_eq!(docs.documentation, "# global\n\n");

        let my_module = &docs.sub_modules[0];

        assert_eq!(my_module.name, "global::my_module");
        pretty_assertions::assert_eq!(
            my_module.documentation,
            r#"# global::my_module

My own module.


<div markdown="span" style='box-shadow: 0 4px 8px 0 rgba(0,0,0,0.2); padding: 15px; border-radius: 5px;'>

<h2 class="func-name"> <code>fn</code> hello_world </h2>

```rust,ignore
fn hello_world()
```

<details>
<summary markdown="span"> details </summary>

A function that prints to stdout.
</details>

</div>
</br>

<div markdown="span" style='box-shadow: 0 4px 8px 0 rgba(0,0,0,0.2); padding: 15px; border-radius: 5px;'>

<h2 class="func-name"> <code>fn</code> add </h2>

```rust,ignore
fn add(a: int, b: int) -> int
```

<details>
<summary markdown="span"> details </summary>

A function that adds two integers together.
</details>

</div>
</br>
"#
        );
    }
}
