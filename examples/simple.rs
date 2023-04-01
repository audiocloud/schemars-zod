use std::collections::HashMap;

use chrono::{DateTime, Utc};
use schemars::{JsonSchema, schema_for};

use schamars_zod::{convert, merge_schemas};

fn main() {
  #[derive(schemars::JsonSchema)]
  struct MyStruct {
    a: String,
    b: u32,
  }

  #[derive(schemars::JsonSchema)]
  #[serde(rename_all = "camelCase")]
  struct MyOtherStruct {
    x: f64,
    y: f64,
    other: MyStruct,
    more: Vec<MyStruct>,
    more_more: HashMap<String, MyStruct>,
    time: DateTime<Utc>,
  }

  let merged = merge_schemas(vec![schema_for!(MyStruct), schema_for!(MyOtherStruct)].into_iter());
  println!("{}", serde_json::to_string_pretty(&merged).unwrap());

  let converted = convert(merged);
  println!("{}", converted.into_values().collect::<Vec<_>>().join("\n"));
}
