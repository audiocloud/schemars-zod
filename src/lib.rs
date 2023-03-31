//! convert JsonSchema to ZOD schema
use std::collections::HashMap;

use schemars::schema::{ArrayValidation, InstanceType, ObjectValidation, RootSchema, Schema, SchemaObject, SingleOrVec};

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

pub fn convert(schema: RootSchema) -> String {
  let mut definitions = HashMap::<String, String>::new();

  for (id, definition) in schema.definitions {
    add_converted_schema(&mut definitions, id, definition.into_object());
  }

  definitions.into_values().collect::<Vec<_>>().join("\n")
}

fn add_converted_schema(definitions: &mut HashMap<String, String>, id: String, schema: SchemaObject) {
  let mut rv = String::new();

  let Some(generated) = convert_schema_object_to_zod(schema) else { return; };

  rv.push_str(&format!("const {id} = {generated};\n"));
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
