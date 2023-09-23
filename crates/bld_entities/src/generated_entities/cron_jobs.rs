//! `SeaORM` Entity. Generated by sea-orm-codegen 0.12.2

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "cron_jobs")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub pipeline_id: String,
    pub schedule: String,
    pub is_default: bool,
    pub date_created: DateTime,
    pub date_updated: Option<DateTime>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::cron_job_environment_variables::Entity")]
    CronJobEnvironmentVariables,
    #[sea_orm(has_many = "super::cron_job_variables::Entity")]
    CronJobVariables,
    #[sea_orm(
        belongs_to = "super::pipeline::Entity",
        from = "Column::PipelineId",
        to = "super::pipeline::Column::Id",
        on_update = "NoAction",
        on_delete = "NoAction"
    )]
    Pipeline,
}

impl Related<super::cron_job_environment_variables::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::CronJobEnvironmentVariables.def()
    }
}

impl Related<super::cron_job_variables::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::CronJobVariables.def()
    }
}

impl Related<super::pipeline::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Pipeline.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
