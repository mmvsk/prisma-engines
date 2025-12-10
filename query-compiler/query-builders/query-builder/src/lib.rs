use psl::schema_ast::ast::FieldArity;
use query_structure::{
    AggregationSelection, FieldSelection, Filter, Model, Placeholder, PrismaValue, QueryArguments, RecordFilter,
    RelationField, RelationLoadStrategy, ScalarCondition, ScalarField, SelectedField, SelectionResult, TypeIdentifier,
    WriteArgs,
};
use serde::Serialize;
use serde_repr::Serialize_repr;
use std::collections::BTreeMap;
use std::fmt::Formatter;
use std::{collections::HashMap, fmt};

mod query_arguments_ext;

pub use query_arguments_ext::QueryArgumentsExt;
use query_template::{Fragment, PlaceholderFormat};

pub trait QueryBuilder {
    fn build_get_records(
        &self,
        model: &Model,
        query_arguments: QueryArguments,
        selected_fields: &FieldSelection,
        relation_load_strategy: RelationLoadStrategy,
    ) -> Result<Vec<DbQuery>, Box<dyn std::error::Error + Send + Sync>>;

    /// Retrieve related records through an M2M relation.
    #[cfg(feature = "relation_joins")]
    fn build_get_related_records(
        &self,
        linkage: RelationLinkage,
        query_arguments: QueryArguments,
        selected_fields: &FieldSelection,
    ) -> Result<GetRelatedRecordsQuery, Box<dyn std::error::Error + Send + Sync>>;

    fn build_aggregate(
        &self,
        model: &Model,
        args: QueryArguments,
        selections: &[AggregationSelection],
        group_by: Vec<ScalarField>,
        having: Option<Filter>,
    ) -> Result<DbQuery, Box<dyn std::error::Error + Send + Sync>>;

    fn build_create_record(
        &self,
        model: &Model,
        args: WriteArgs,
        selected_fields: &FieldSelection,
    ) -> Result<CreateRecord, Box<dyn std::error::Error + Send + Sync>>;

    fn build_inserts(
        &self,
        model: &Model,
        args: Vec<WriteArgs>,
        skip_duplicates: bool,
        selected_fields: Option<&FieldSelection>,
    ) -> Result<Vec<DbQuery>, Box<dyn std::error::Error + Send + Sync>>;

    fn build_update(
        &self,
        model: &Model,
        record_filter: RecordFilter,
        args: WriteArgs,
        selected_fields: Option<&FieldSelection>,
    ) -> Result<DbQuery, Box<dyn std::error::Error + Send + Sync>>;

    fn build_updates(
        &self,
        model: &Model,
        record_filter: RecordFilter,
        args: WriteArgs,
        selected_fields: Option<&FieldSelection>,
        limit: Option<usize>,
    ) -> Result<Vec<DbQuery>, Box<dyn std::error::Error + Send + Sync>>;

    fn build_upsert(
        &self,
        model: &Model,
        filter: Filter,
        create_args: WriteArgs,
        update_args: WriteArgs,
        selected_fields: &FieldSelection,
        unique_constraints: &[ScalarField],
    ) -> Result<DbQuery, Box<dyn std::error::Error + Send + Sync>>;

    fn build_m2m_connect(
        &self,
        parent_field: RelationField,
        parent: PrismaValue,
        child: PrismaValue,
    ) -> Result<DbQuery, Box<dyn std::error::Error + Send + Sync>>;

    fn build_m2m_disconnect(
        &self,
        field: RelationField,
        parent_id: &SelectionResult,
        child_ids: &[SelectionResult],
    ) -> Result<DbQuery, Box<dyn std::error::Error + Send + Sync>>;

    fn build_delete(
        &self,
        model: &Model,
        filter: RecordFilter,
        selected_fields: Option<&FieldSelection>,
    ) -> Result<DbQuery, Box<dyn std::error::Error + Send + Sync>>;

    fn build_deletes(
        &self,
        model: &Model,
        filter: RecordFilter,
        limit: Option<usize>,
    ) -> Result<Vec<DbQuery>, Box<dyn std::error::Error + Send + Sync>>;

    fn build_raw(
        &self,
        model: Option<&Model>,
        inputs: HashMap<String, PrismaValue>,
        query_type: Option<String>,
    ) -> Result<DbQuery, Box<dyn std::error::Error + Send + Sync>>;
}

/// An insertion operation for a record in the database.
pub struct CreateRecord {
    /// The insert query to run in order to create the record.
    pub insert_query: DbQuery,
    /// The query to run prior to the insert in order to create default column values.
    /// This is used in some cases where the database does not support returning default values.
    pub select_defaults: Option<CreateRecordDefaultsQuery>,
    /// The field in the model of the record that corresponds to the last inserted ID, if
    /// required by the database.
    pub last_insert_id_field: Option<ScalarField>,
    /// The values to merge into the resulting record after insertion. These are inferred from the
    /// input arguments.
    pub merge_values: Vec<(SelectedField, PrismaValue)>,
}

/// A query that retrieves default values needed for an insert operation.
pub struct CreateRecordDefaultsQuery {
    /// The query that returns the default values.
    pub query: DbQuery,
    /// The fields that are selected in the query and their corresponding placeholders.
    /// These placeholders are referred to by the subsequent insert query.
    pub field_placeholders: Vec<(ScalarField, Placeholder)>,
}

/// A query that retrieves related records through an M2M relation.
#[cfg(feature = "relation_joins")]
pub struct GetRelatedRecordsQuery {
    /// The query that retrieves the related records.
    pub query: DbQuery,
    /// The alias used for the linking field in the query.
    pub linking_field_alias: String,
}

#[derive(Debug)]
pub struct ConditionalLink {
    field: ScalarField,
    conditions: Vec<ScalarCondition>,
}

impl ConditionalLink {
    pub fn new(field: ScalarField, conditions: Vec<ScalarCondition>) -> Self {
        Self { field, conditions }
    }

    pub fn field(&self) -> &ScalarField {
        &self.field
    }

    pub fn into_field_and_conditions(self) -> (ScalarField, Vec<ScalarCondition>) {
        (self.field, self.conditions)
    }
}

#[derive(Debug)]
pub struct RelationLinkage {
    parent_field: RelationField,
    conditions: BTreeMap<ScalarField, Vec<ScalarCondition>>,
}

impl RelationLinkage {
    pub fn new(field: RelationField, links: Vec<ConditionalLink>) -> Self {
        Self {
            parent_field: field,
            conditions: links
                .into_iter()
                .map(ConditionalLink::into_field_and_conditions)
                .collect(),
        }
    }

    pub fn parent_field(&self) -> &RelationField {
        &self.parent_field
    }

    pub fn add_condition(&mut self, field: ScalarField, condition: ScalarCondition) {
        self.conditions.entry(field).or_default().push(condition);
    }

    pub fn into_parent_field_and_conditions(
        self,
    ) -> (
        RelationField,
        impl Iterator<Item = (ScalarField, Vec<ScalarCondition>)> + fmt::Debug,
    ) {
        (self.parent_field, self.conditions.into_iter())
    }
}

impl fmt::Display for RelationLinkage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}@{}",
            self.parent_field.relation().name(),
            self.parent_field.model().name()
        )
    }
}

/// Column type information for driver adapters.
///
/// These values match the TypeScript `ColumnTypeEnum` in `@prisma/driver-adapter-utils`.
/// The numeric representation is important for serialization compatibility with the
/// TypeScript query interpreter.
#[derive(Debug, Clone, Copy, Serialize_repr)]
#[repr(u8)]
pub enum ColumnType {
    // Scalars
    Int32 = 0,
    Int64 = 1,
    Float = 2,
    Double = 3,
    Numeric = 4,
    Boolean = 5,
    Character = 6,
    Text = 7,
    Date = 8,
    Time = 9,
    DateTime = 10,
    Json = 11,
    Enum = 12,
    Bytes = 13,
    Set = 14,
    Uuid = 15,
    // Arrays (64+)
    Int32Array = 64,
    Int64Array = 65,
    FloatArray = 66,
    DoubleArray = 67,
    NumericArray = 68,
    BooleanArray = 69,
    CharacterArray = 70,
    TextArray = 71,
    DateArray = 72,
    TimeArray = 73,
    DateTimeArray = 74,
    JsonArray = 75,
    EnumArray = 76,
    BytesArray = 77,
    UuidArray = 78,
    // Custom
    UnknownNumber = 128,
}

impl ColumnType {
    /// Convert a Prisma TypeIdentifier and arity to a ColumnType.
    ///
    /// Returns `None` for types that don't have a direct mapping (e.g., Unsupported, Extension).
    pub fn from_type_identifier(type_id: &TypeIdentifier, arity: FieldArity) -> Option<Self> {
        let is_list = arity.is_list();

        let column_type = match type_id {
            TypeIdentifier::String => {
                if is_list {
                    ColumnType::TextArray
                } else {
                    ColumnType::Text
                }
            }
            TypeIdentifier::Int => {
                if is_list {
                    ColumnType::Int32Array
                } else {
                    ColumnType::Int32
                }
            }
            TypeIdentifier::BigInt => {
                if is_list {
                    ColumnType::Int64Array
                } else {
                    ColumnType::Int64
                }
            }
            TypeIdentifier::Float => {
                if is_list {
                    ColumnType::DoubleArray
                } else {
                    ColumnType::Double
                }
            }
            TypeIdentifier::Decimal => {
                if is_list {
                    ColumnType::NumericArray
                } else {
                    ColumnType::Numeric
                }
            }
            TypeIdentifier::Boolean => {
                if is_list {
                    ColumnType::BooleanArray
                } else {
                    ColumnType::Boolean
                }
            }
            TypeIdentifier::Enum(_) => {
                if is_list {
                    ColumnType::EnumArray
                } else {
                    ColumnType::Enum
                }
            }
            TypeIdentifier::UUID => {
                if is_list {
                    ColumnType::UuidArray
                } else {
                    ColumnType::Uuid
                }
            }
            TypeIdentifier::Json => {
                if is_list {
                    ColumnType::JsonArray
                } else {
                    ColumnType::Json
                }
            }
            TypeIdentifier::DateTime => {
                if is_list {
                    ColumnType::DateTimeArray
                } else {
                    ColumnType::DateTime
                }
            }
            TypeIdentifier::Bytes => {
                if is_list {
                    ColumnType::BytesArray
                } else {
                    ColumnType::Bytes
                }
            }
            // Extension and Unsupported types don't have a direct mapping
            TypeIdentifier::Extension(_) | TypeIdentifier::Unsupported => return None,
        };

        Some(column_type)
    }
}

/// Extract column types from a FieldSelection.
///
/// Returns a vector of optional ColumnTypes corresponding to each selected field.
/// Returns `None` for fields that don't have a direct column type mapping (e.g., composites).
pub fn extract_column_types(field_selection: &FieldSelection) -> Vec<Option<ColumnType>> {
    field_selection
        .type_identifiers_with_arities()
        .into_iter()
        .map(|(type_id, arity)| ColumnType::from_type_identifier(&type_id, arity))
        .collect()
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum DbQuery {
    #[serde(rename_all = "camelCase")]
    RawSql {
        sql: String,
        args: Vec<PrismaValue>,
        arg_types: Vec<ArgType>,
        #[serde(skip_serializing_if = "Option::is_none")]
        column_types: Option<Vec<Option<ColumnType>>>,
    },
    #[serde(rename_all = "camelCase")]
    TemplateSql {
        fragments: Vec<Fragment>,
        args: Vec<PrismaValue>,
        arg_types: Vec<DynamicArgType>,
        placeholder_format: PlaceholderFormat,
        chunkable: Chunkable,
        #[serde(skip_serializing_if = "Option::is_none")]
        column_types: Option<Vec<Option<ColumnType>>>,
    },
}

impl DbQuery {
    pub fn params(&self) -> &[PrismaValue] {
        match self {
            DbQuery::RawSql { args: params, .. } => params,
            DbQuery::TemplateSql { args: params, .. } => params,
        }
    }
}

impl fmt::Display for DbQuery {
    /// Should only be used for debugging, unit testing and playground CLI output.
    /// The placeholder syntax does not attempt to match any actual SQL flavour.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            DbQuery::RawSql { sql, .. } => {
                write!(formatter, "{sql}")?;
            }
            DbQuery::TemplateSql { fragments, .. } => {
                let placeholder_format = PlaceholderFormat {
                    prefix: "$",
                    has_numbering: true,
                };
                let mut number = 1;
                for fragment in fragments {
                    match fragment {
                        Fragment::StringChunk { chunk } => {
                            write!(formatter, "{chunk}")?;
                        }
                        Fragment::Parameter => {
                            placeholder_format.write(formatter, &mut number)?;
                        }
                        Fragment::ParameterTuple => {
                            write!(formatter, "[")?;
                            placeholder_format.write(formatter, &mut number)?;
                            write!(formatter, "]")?;
                        }
                        Fragment::ParameterTupleList { .. } => {
                            write!(formatter, "[(")?;
                            placeholder_format.write(formatter, &mut number)?;
                            write!(formatter, ")]")?;
                        }
                    };
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
#[serde(tag = "arity", rename_all = "camelCase")]
pub enum DynamicArgType {
    Tuple {
        elements: Vec<ArgType>,
    },
    #[serde(untagged)]
    Single {
        #[serde(flatten)]
        r#type: ArgType,
    },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArgType {
    pub arity: Arity,
    pub scalar_type: ArgScalarType,
    pub db_type: Option<String>,
}

impl ArgType {
    pub fn new(arity: Arity, scalar_type: ArgScalarType, db_type: Option<String>) -> Self {
        Self {
            arity,
            scalar_type,
            db_type,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Arity {
    Scalar,
    List,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ArgScalarType {
    String,
    Int,
    #[serde(rename = "bigint")]
    BigInt,
    Float,
    Decimal,
    Boolean,
    Enum,
    Uuid,
    Json,
    #[serde(rename = "datetime")]
    DateTime,
    Bytes,
    Unknown,
}

/// Indicates whether the parameters of this query can be chunked into smaller queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(into = "bool")]
pub enum Chunkable {
    Yes,
    No,
}

impl From<Chunkable> for bool {
    fn from(chunkable: Chunkable) -> Self {
        matches!(chunkable, Chunkable::Yes)
    }
}

impl From<&QueryArguments> for Chunkable {
    fn from(args: &QueryArguments) -> Self {
        if !args.order_by.is_empty()
            || args.cursor.is_some()
            || args.has_unbatchable_filters()
            || args.has_unbatchable_ordering()
        {
            Chunkable::No
        } else {
            Chunkable::Yes
        }
    }
}

impl From<&Filter> for Chunkable {
    fn from(filter: &Filter) -> Self {
        if filter.can_batch() {
            Chunkable::Yes
        } else {
            Chunkable::No
        }
    }
}
