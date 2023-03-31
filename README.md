# schemars-zod

**experimental** `schamars` to `zod` converter.

Contains A few functions to aid Zod schema generation from rust types annotated with schemars.

## usage

given these types:

```rust
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
  time: DateTime<Utc>, // from chrono crate
}
```

This code will corresponding Zod types:

```rust
let merged = merge_schemas(vec![schema_for!(MyStruct), schema_for!(MyOtherStruct)].into_iter());
let converted = convert(merged);
println!("{}", converted);
```

And output:

```ts
export const MyStruct = z.object({a: z.string(), b: z.number().int(),});
export type MyStruct = z.infer<typeof MyStruct>;

export const MyOtherStruct = z.object({
    more: z.array(z.lazy(() => MyStruct)),
    moreMore: z.record(z.lazy(() => MyStruct)),
    other: z.lazy(() => MyStruct),
    time: z.coerce.date(),
    x: z.number(),
    y: z.number(),
});
export type MyOtherStruct = z.infer<typeof MyOtherStruct>;
```