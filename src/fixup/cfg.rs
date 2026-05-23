//! placeholder for S6 Task; replaced in subsequent task

#[derive(Debug)]
pub struct CfgPredicate;

pub struct CfgContext<'a> {
    _marker: std::marker::PhantomData<&'a ()>,
}
