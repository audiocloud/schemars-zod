//! convert JsonSchema to ZOD schema
use std::collections::HashMap;

use schemars::schema::{ArrayValidation, InstanceType, ObjectValidation, RootSchema, Schema, SchemaObject, SingleOrVec};

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
/// use schamars_zod::merge_schemas;
///
/// #[derive(schemars::JsonSchema)]
/// struct MyStruct {/* ... */}
///
///#[derive(schemars::JsonSchema)]
/// struct MyOtherStruct {/* ... */}
/// let merged = merge_schemas(vec![schema_for!(MyStruct), schema_for!(MyOtherStruct)].into_iter());
/// ```
pub fn merge_schemas(schemas: impl Iterator<Item=RootSchema>) -> RootSchema {
  let mut merged = RootSchema::default();
  for schema in schemas {
    let Some(id) = schema.schema.metadata.as_ref().and_then(|m| m.title.as_ref())
      else { continue; };

    for (id, definition) in schema.definitions {
      merged.definitions.insert(id, definition);
    }

    merged.definitions.insert(id.to_owned(), Schema::Object(schema.schema));
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
/// use schamars_zod::{convert, merge_schemas};
///
/// #[derive(schemars::JsonSchema)]
/// struct MyStruct {/* ... */}
///
///#[derive(schemars::JsonSchema)]
/// struct MyOtherStruct {/* ... */}
///
/// let converted = convert(merge_schemas(vec![schema_for!(MyStruct), schema_for!(MyOtherStruct)].into_iter()));
/// ```
pub fn convert(schema: RootSchema) -> HashMap::<String, String> {
  let mut definitions = HashMap::new();

  for (id, definition) in schema.definitions {
    add_converted_schema(&mut definitions, id, definition.into_object());
  }

  definitions
}

fn add_converted_schema(definitions: &mut HashMap<String, String>, id: String, schema: SchemaObject) {
  let mut rv = String::new();

  let Some(generated) = convert_schema_object_to_zod(schema) else { return; };

  rv.push_str(&format!("export const {id} = {generated};\n"));
  rv.push_str(&format!("export type {id} = z.infer<typeof {id}>;\n"));

  definitions.insert(id, rv);
}

fn convert_schema_object_to_zod(schema: SchemaObject) -> Option<String> {
  if let Some(reference) = schema.reference.as_ref() {
    let reference = reference.replace("#/definitions/", "");
    return Some(format!("z.lazy(() => {reference})"));
  }

  let Some(instance_type) = schema.instance_type.as_ref() else { return None; };

  convert_schema_type_to_zod(instance_type, &schema)
}

fn convert_schema_type_to_zod(instance_type: &SingleOrVec<InstanceType>, schema: &SchemaObject) -> Option<String> {
  match instance_type {
    SingleOrVec::Single(single_type) => {
      convert_single_instance_type_schema_to_zod(single_type, &schema)
    }
    SingleOrVec::Vec(multiple_types) => {
      convert_union_type_schema_to_zod(multiple_types, &schema)
    }
  }
}

fn convert_single_instance_type_schema_to_zod(instance_type: &Box<InstanceType>, schema: &SchemaObject) -> Option<String> {
  match instance_type.as_ref() {
    InstanceType::Null => { Some(format!("z.null()")) }
    InstanceType::Boolean => { Some(format!("z.boolean()")) }
    InstanceType::Object => { convert_object_type_to_zod(schema.object.as_ref().unwrap(), schema) }
    InstanceType::Array => { convert_array_type_to_zod(schema.array.as_ref().unwrap(), schema) }
    InstanceType::Number => { Some(format!("z.number()")) }
    InstanceType::String => {
      if matches!(schema.format.as_ref(), Some(format) if format == "date-time") {
        return Some(format!("z.coerce.date()"));
      }
      Some(format!("z.string()"))
    }
    InstanceType::Integer => { Some(format!("z.number().int()")) }
  }
}

fn convert_array_type_to_zod(array_type: &Box<ArrayValidation>, schema: &SchemaObject) -> Option<String> {
  let Some(items) = array_type.items.as_ref() else { return None; };

  let mut rv = String::new();
  rv.push_str("z.array(");
  let Some(generated) = convert_schema_or_ref_to_zod(items) else { return None; };
  rv.push_str(&format!("{generated})"));
  Some(rv)
}

fn convert_schema_or_ref_to_zod(schema: &SingleOrVec<Schema>) -> Option<String> {
  match schema {
    SingleOrVec::Single(schema_or_ref) => {
      convert_schema_object_to_zod(schema_or_ref.clone().into_object())
    }
    SingleOrVec::Vec(schemas) => {
      let mut rv = String::new();
      rv.push_str("z.union([");
      for schema in schemas {
        if let Some(schema) = convert_schema_object_to_zod(schema.clone().into_object()) {
          rv.push_str(&format!("{schema}, ", ));
        }
      }
      rv.push_str("])");
      Some(rv)
    }
  }
}

fn convert_object_type_to_zod(object_type: &Box<ObjectValidation>, schema: &SchemaObject) -> Option<String> {
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
    rv.push_str(&format!("{property_name}: {property_type}, ", ));
  }

  rv.push_str("})");

  Some(rv)
}

fn convert_union_type_schema_to_zod(instance_types: &Vec<InstanceType>, schema: &SchemaObject) -> Option<String> {
  let mut rv = String::new();

  rv.push_str("z.union([");
  for instance_type in instance_types {
    let Some(generated) = convert_single_instance_type_schema_to_zod(&Box::new(instance_type.clone()), schema) else { return None; };
    rv.push_str(&format!("{generated}, "));
  }

  rv.push_str("])");

  Some(rv)
}
