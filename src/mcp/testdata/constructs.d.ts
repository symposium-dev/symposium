type Config = {
  host: string;
};

type Json = string | Json[];

type Merged = {
  a?: string;
} & {
  b?: number;
};

type Shape = {
  Circle: number;
} | {
  Square: number;
};

declare const constructs: {
  /** Input is a $ref to a named model, the shape pydantic emits for a root model. */
  pydantic_root_model(): Promise<unknown>;
  /** A serde externally-tagged enum, the most common schemars-generated $def. */
  schemars_tagged_enum(params: {
    shape: Shape;
  }): Promise<unknown>;
  /** oneOf beside properties: the 'provide one of these' idiom. */
  either_path_or_url(params?: unknown): Promise<unknown>;
  /** allOf alongside local properties; both belong in the result. */
  all_of_with_siblings(params: {
    shared: number;
  }): Promise<unknown>;
  /** Defines #/$defs/Config. Generators re-emit $defs per tool, so the name is not unique across a server. */
  config_first(params: {
    config: Config;
  }): Promise<unknown>;
  /** Defines #/$defs/Config with a different body. Must not collapse into the first. */
  config_second(params: {
    config: Config;
  }): Promise<unknown>;
  /** properties without a type keyword; type is not required by JSON Schema. */
  no_declared_type(params: unknown): Promise<unknown>;
  /** The pydantic optional spelling. A control: this one already works. */
  nullable_via_any_of(params: {
    revision: string | null;
  }): Promise<unknown>;
  /** Both tuple spellings. zod emits the first on draft-07 targets and the second on 2020-12. */
  tuple_shapes(params?: {
    draft07?: unknown[];
    modern?: never[];
  }): Promise<unknown>;
  /** Self-reference through a union, the shape a JSON-value type takes. */
  recursive_defs(params: {
    tree: Json;
  }): Promise<unknown>;
  /** A $def whose body is an intersection rather than a single object. */
  merged_object_def(params: {
    merged: Merged;
  }): Promise<unknown>;
};
