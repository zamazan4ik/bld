//! `SeaORM` Entity. Generated by sea-orm-codegen 0.12.2

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "high_availability_members")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: i32,
    pub snapshot_id: i32,
    pub date_created: DateTime,
    pub date_updated: Option<DateTime>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::high_availability_snapshot::Entity",
        from = "Column::SnapshotId",
        to = "super::high_availability_snapshot::Column::Id",
        on_update = "NoAction",
        on_delete = "NoAction"
    )]
    HighAvailabilitySnapshot,
}

impl Related<super::high_availability_snapshot::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::HighAvailabilitySnapshot.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}