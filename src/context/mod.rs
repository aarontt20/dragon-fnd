use crate::Error;

#[derive(Debug)]
pub struct AppContext<C> {
    config: C,
}

impl<C> AppContext<C> {
    pub fn config(&self) -> &C {
        &self.config
    }
}

impl AppContext<()> {
    pub fn builder() -> AppContextBuilder<()> {
        AppContextBuilder { config: None }
    }
}

#[derive(Debug)]
#[must_use = "builders do nothing until .build() is called"]
pub struct AppContextBuilder<C> {
    config: Option<C>,
}

impl AppContextBuilder<()> {
    pub fn with_config<C>(self, config: C) -> AppContextBuilder<C> {
        AppContextBuilder {
            config: Some(config),
        }
    }
}

impl<C> AppContextBuilder<C> {
    pub fn build(self) -> Result<AppContext<C>, Error> {
        Ok(AppContext {
            config: self.config.ok_or(Error::MissingConfig)?,
        })
    }
}
