//! convert JsonSchema to ZOD schema
use std::collections::{HashMap, HashSet};

use schemars::schema::{
    ArrayValidation, InstanceType, ObjectValidation, RootSchema, Schema, SchemaObject, SingleOrVec,
};

/// Merge multiple [schemars::schema::RootSchema] into a single [schemars::schema::RootSchema].
///
/// Schemars macro [schemars::schema_for!] will always generate a RootSchema. This works if each
/// type is independently processed. In ZOD schemas however, it's a common practice to define all
/// types in a single file, and then import them as needed. This function will merge multiple
/// results of the `schema_for!` macro into a single RootSchema, simplifying multiple definitions
/// of the same schema.
///
/// # Arguments
///
/// * `schemas`: An iterator of RootSchema's to merge. BYOI (Bring Your Own Iterator)
///
/// returns: RootSchema - A single RootSchema containing all definitions from the input schemas.
///
/// # Examples
///
/// ```
/// use schemars::schema_for;
/// use schemars_zod::merge_schemas;
///
/// #[derive(schemars::JsonSchema)]
/// struct MyStruct {/* ... */}
///
///#[derive(schemars::JsonSchema)]
/// struct MyOtherStruct {/* ... */}
/// let merged = merge_schemas(vec![schema_for!(MyStruct), schema_for!(MyOtherStruct)].into_iter());
/// ```
pub fn merge_schemas(schemas: impl Iterator<Item = RootSchema>) -> RootSchema {
    let mut merged = RootSchema::default();
    for schema in schemas {
        let Some(id) = schema.schema.metadata.as_ref().and_then(|m| m.title.as_ref())
      else { continue; };

        for (id, definition) in schema.definitions {
            merged.definitions.insert(id, definition);
        }

        merged
            .definitions
            .insert(id.to_owned(), Schema::Object(schema.schema));
    }

    merged
}

/// Convert a [schemars::schema::RootSchema] to a HashMap of stringified ZOD schemas.
///
/// Only definitions inside the RootSchema will be converted, the root schema itself will be ignored.
///
/// # Arguments
///
/// * `schema`: Schema to convert
///
/// returns: HashMap<String, String> - A HashMap of stringified ZOD schemas, keyed by the definition name.
///
/// # Examples
///
/// ```
/// use schemars::schema_for;
/// use schemars_zod::{convert, merge_schemas};
///
/// #[derive(schemars::JsonSchema)]
/// struct MyStruct {/* ... */}
///
///#[derive(schemars::JsonSchema)]
/// struct MyOtherStruct {/* ... */}
///
/// let converted = convert(merge_schemas(vec![schema_for!(MyStruct), schema_for!(MyOtherStruct)].into_iter()));
/// ```
pub fn convert(schema: RootSchema) -> HashMap<String, String> {
    let mut definitions = HashMap::new();

    for (id, definition) in schema.definitions {
        add_converted_schema(&mut definitions, id, definition.into_object());
    }

    definitions
}

fn add_converted_schema(
    definitions: &mut HashMap<String, String>,
    id: String,
    schema: SchemaObject,
) {
    let mut rv = String::new();

    let Some(generated) = convert_schema_object_to_zod(schema) else { return; };

    rv.push_str(&format!("export const {id} = {generated};\n"));
    rv.push_str(&format!("export type {id} = z.infer<typeof {id}>;\n"));

    definitions.insert(id, rv);
}

fn convert_schema_object_to_zod(schema: SchemaObject) -> Option<String> {
    // handle references
    if let Some(reference) = schema.reference.as_ref() {
        let reference = reference.replace("#/definitions/", "");
        return Some(format!("z.lazy(() => {reference})"));
    }

    // handle ordinary value disjoint unions / enums
    if let Some(enum_values) = schema.enum_values.as_ref() {
        if enum_values.len() == 1 {
            return Some(format!(
                "z.literal({})",
                serde_json::to_string_pretty(enum_values.first().unwrap()).unwrap()
            ));
        }

        let mut rv = String::new();
        rv.push_str("z.enum([");
        for value in enum_values {
            rv.push_str(&format!(
                "{}, ",
                serde_json::to_string_pretty(&value).unwrap()
            ));
        }
        rv.push_str("])");

        return Some(rv);
    }

    // handle tagged / untagged unions
    if let Some(one_of) = schema.subschemas.as_ref().and_then(|x| x.one_of.as_ref()) {
        let mut rv = if let Some(field) = all_schemas_share_a_field(one_of) {
            format!("z.discriminatedUnion('{field}', [")
        } else {
            format!("z.union([")
        };

        for schema in one_of {
            let Some(generated) = convert_schema_object_to_zod(schema.clone().into_object()) else { continue; };
            rv.push_str(&format!("{generated}, "));
        }

        rv.push_str("])");
        return Some(rv);
    }

    let Some(instance_type) = schema.instance_type.as_ref() else { return None; };

    convert_schema_type_to_zod(instance_type, &schema)
}

fn all_schemas_share_a_field(any_of: &[Schema]) -> Option<String> {
    let mut results = Vec::<HashSet<String>>::new();
    for schema in any_of {
        let schema = schema.clone().into_object();
        if schema.instance_type.as_ref()
            == Some(&SingleOrVec::Single(Box::new(InstanceType::Object)))
        {
            results.push(schema.object.unwrap().properties.keys().cloned().collect());
        } else {
            results.push(HashSet::default());
        }
    }

    results.first().and_then(|first_props| {
        let found = first_props
            .iter()
            .filter(|prop_name| {
                results
                    .iter()
                    .skip(1)
                    .all(|props| props.contains(*prop_name))
            })
            .cloned()
            .collect::<HashSet<_>>();

        if found.contains("type") {
          Some("type".to_owned())
        } else if found.contains("kind") {
          Some("kind".to_owned())
        } else {
          found.iter().next().map(|x| x.to_owned())
        }
    })
}

fn convert_schema_type_to_zod(
    instance_type: &SingleOrVec<InstanceType>,
    schema: &SchemaObject,
) -> Option<String> {
    match instance_type {
        SingleOrVec::Single(single_type) => {
            convert_single_instance_type_schema_to_zod(single_type, &schema)
        }
        SingleOrVec::Vec(multiple_types) => {
            convert_union_type_schema_to_zod(multiple_types, &schema)
        }
    }
}

fn convert_single_instance_type_schema_to_zod(
    instance_type: &Box<InstanceType>,
    schema: &SchemaObject,
) -> Option<String> {
    if let Some(literal_value) = schema.const_value.as_ref() {
        return Some(format!(
            "z.literal({})",
            serde_json::to_string_pretty(literal_value).unwrap()
        ));
    }

    match instance_type.as_ref() {
        InstanceType::Null => Some(format!("z.null()")),
        InstanceType::Boolean => Some(format!("z.boolean()")),
        InstanceType::Object => convert_object_type_to_zod(schema.object.as_ref().unwrap(), schema),
        InstanceType::Array => convert_array_type_to_zod(schema.array.as_ref().unwrap(), schema),
        InstanceType::Number => Some(format!("z.number()")),
        InstanceType::String => {
            if matches!(schema.format.as_ref(), Some(format) if format == "date-time") {
                return Some(format!("z.coerce.date()"));
            }
            Some(format!("z.string()"))
        }
        InstanceType::Integer => Some(format!("z.number().int()")),
    }
}

fn convert_array_type_to_zod(
    array_type: &Box<ArrayValidation>,
    schema: &SchemaObject,
) -> Option<String> {
    let Some(items) = array_type.items.as_ref() else { return None; };

    if array_type.min_items.is_some() && array_type.min_items == array_type.max_items {
        convert_schema_or_ref_to_zod(items, "tuple")
    } else {
        let mut rv = String::new();
        rv.push_str("z.array(");
        let Some(generated) = convert_schema_or_ref_to_zod(items, "union") else { return None; };
        rv.push_str(&format!("{generated})"));
        Some(rv)
    }
}

fn convert_schema_or_ref_to_zod(schema: &SingleOrVec<Schema>, zod_mode: &str) -> Option<String> {
    match schema {
        SingleOrVec::Single(schema_or_ref) => {
            convert_schema_object_to_zod(schema_or_ref.clone().into_object())
        }
        SingleOrVec::Vec(schemas) => {
            if schemas.len() == 1 {
                return convert_schema_object_to_zod(
                    schemas.first().unwrap().clone().into_object(),
                );
            }

            let mut rv = String::new();
            rv.push_str(&format!("z.{zod_mode}(["));
            for schema in schemas {
                if let Some(schema) = convert_schema_object_to_zod(schema.clone().into_object()) {
                    rv.push_str(&format!("{schema}, ",));
                }
            }
            rv.push_str("])");
            Some(rv)
        }
    }
}

fn convert_object_type_to_zod(
    object_type: &Box<ObjectValidation>,
    schema: &SchemaObject,
) -> Option<String> {
    let mut rv = String::new();

    // are we additional objects and no properties? if so, we are a record
    if object_type.additional_properties.is_some() && object_type.properties.is_empty() {
        let Some(additional_properties) = object_type.additional_properties.as_ref() else { return None; };
        let Some(additional_properties) = convert_schema_object_to_zod(additional_properties.clone().into_object()) else { return None; };
        return Some(format!("z.record({additional_properties})"));
    }

    rv.push_str("z.object({");

    for (property_name, property) in &object_type.properties {
        let Some(property_type) = convert_schema_object_to_zod(property.clone().into_object()) else { return None; };
        rv.push_str(&format!("{property_name}: {property_type}, ",));
    }

    rv.push_str("})");

    Some(rv)
}

fn convert_union_type_schema_to_zod(
    instance_types: &Vec<InstanceType>,
    schema: &SchemaObject,
) -> Option<String> {
    let mut rv = String::new();

    rv.push_str("z.union([");
    for instance_type in instance_types {
        let Some(generated) = convert_single_instance_type_schema_to_zod(&Box::new(instance_type.clone()), schema) else { return None; };
        rv.push_str(&format!("{generated}, "));
    }

    rv.push_str("])");

    Some(rv)
}
