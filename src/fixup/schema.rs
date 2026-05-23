//! placeholder for S6 Task; replaced in subsequent task

#[derive(Debug, Default)]
pub struct FixupBody;

#[derive(Debug, Default)]
pub struct FixupConfig;

#[derive(Debug)]
pub enum EntryPoints {
    Auto(bool),
    Named(Vec<String>),
}

#[derive(Debug)]
pub struct SdistFixup;
