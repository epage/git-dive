use anyhow::Context as _;
use proc_exit::WithCodeResultExt;

pub fn dump_config(output_path: &std::path::Path) -> proc_exit::ExitResult {
    let cwd = std::env::current_dir().with_code(proc_exit::Code::USAGE_ERR)?;
    let repo = git2::Repository::discover(&cwd).with_code(proc_exit::Code::USAGE_ERR)?;

    let config = crate::config::Config::with_repo(&repo).with_code(proc_exit::Code::CONFIG_ERR)?;
    let output = config.dump([&crate::blame::THEME as &dyn ReflectField]);

    if output_path == std::path::Path::new("-") {
        use std::io::Write;
        std::io::stdout().write_all(output.as_bytes())?;
    } else {
        std::fs::write(output_path, &output)?;
    }

    Ok(())
}

pub struct Config {
    config: Box<dyn ConfigSource>,
}

impl Config {
    pub fn with_repo(repo: &git2::Repository) -> anyhow::Result<Self> {
        let config = repo.config().with_context(|| {
            anyhow::format_err!("failed to read config for {}", repo.path().display())
        })?;
        let config = Box::new(config);
        Ok(Self { config })
    }

    pub fn get<F: Field>(&self, field: &F) -> F::Output {
        field.get_from(&self)
    }

    pub fn dump<'f>(&self, fields: impl IntoIterator<Item = &'f dyn ReflectField>) -> String {
        use std::fmt::Write;

        let mut output = String::new();

        let mut prior_section = "";
        for field in fields {
            let (section, name) = field
                .name()
                .split_once('.')
                .unwrap_or_else(|| panic!("field `{}` is missing a section", field.name()));
            if section != prior_section {
                let _ = writeln!(&mut output, "[{}]", section);
                prior_section = section;
            }
            let _ = writeln!(&mut output, "\t{} = {}", name, field.dump(self));
        }

        output
    }
}

pub trait ConfigSource {
    fn name(&self) -> &str;

    fn get_bool(&self, name: &str) -> anyhow::Result<bool>;
    fn get_i32(&self, name: &str) -> anyhow::Result<i32>;
    fn get_i64(&self, name: &str) -> anyhow::Result<i64>;
    fn get_string(&self, name: &str) -> anyhow::Result<String>;
    fn get_path(&self, name: &str) -> anyhow::Result<std::path::PathBuf>;
}

impl ConfigSource for Config {
    fn name(&self) -> &str {
        "git"
    }

    fn get_bool(&self, name: &str) -> anyhow::Result<bool> {
        self.config.get_bool(name)
    }
    fn get_i32(&self, name: &str) -> anyhow::Result<i32> {
        self.config.get_i32(name)
    }
    fn get_i64(&self, name: &str) -> anyhow::Result<i64> {
        self.config.get_i64(name)
    }
    fn get_string(&self, name: &str) -> anyhow::Result<String> {
        self.config.get_string(name)
    }
    fn get_path(&self, name: &str) -> anyhow::Result<std::path::PathBuf> {
        self.config.get_path(name)
    }
}

impl ConfigSource for git2::Config {
    fn name(&self) -> &str {
        "git"
    }

    fn get_bool(&self, name: &str) -> anyhow::Result<bool> {
        self.get_bool(name).map_err(|e| e.into())
    }
    fn get_i32(&self, name: &str) -> anyhow::Result<i32> {
        self.get_i32(name).map_err(|e| e.into())
    }
    fn get_i64(&self, name: &str) -> anyhow::Result<i64> {
        self.get_i64(name).map_err(|e| e.into())
    }
    fn get_string(&self, name: &str) -> anyhow::Result<String> {
        self.get_string(name).map_err(|e| e.into())
    }
    fn get_path(&self, name: &str) -> anyhow::Result<std::path::PathBuf> {
        self.get_path(name).map_err(|e| e.into())
    }
}

pub trait FieldReader<T> {
    fn get_field(&self, name: &str) -> anyhow::Result<T>;
}

impl<C: ConfigSource> FieldReader<bool> for C {
    fn get_field(&self, name: &str) -> anyhow::Result<bool> {
        self.get_bool(name)
            .with_context(|| anyhow::format_err!("failed to read `{}`", name))
    }
}

impl<C: ConfigSource> FieldReader<i32> for C {
    fn get_field(&self, name: &str) -> anyhow::Result<i32> {
        self.get_i32(name)
            .with_context(|| anyhow::format_err!("failed to read `{}`", name))
    }
}

impl<C: ConfigSource> FieldReader<i64> for C {
    fn get_field(&self, name: &str) -> anyhow::Result<i64> {
        self.get_i64(name)
            .with_context(|| anyhow::format_err!("failed to read `{}`", name))
    }
}

impl<C: ConfigSource> FieldReader<String> for C {
    fn get_field(&self, name: &str) -> anyhow::Result<String> {
        self.get_string(name)
            .with_context(|| anyhow::format_err!("failed to read `{}`", name))
    }
}

impl<C: ConfigSource> FieldReader<std::path::PathBuf> for C {
    fn get_field(&self, name: &str) -> anyhow::Result<std::path::PathBuf> {
        self.get_path(name)
            .with_context(|| anyhow::format_err!("failed to read `{}`", name))
    }
}

pub trait Field {
    type Output;

    fn name(&self) -> &'static str;
    fn get_from(&self, config: &Config) -> Self::Output;
}

pub struct RawField<R> {
    name: &'static str,
    _type: std::marker::PhantomData<R>,
}

impl<R> RawField<R> {
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            _type: std::marker::PhantomData,
        }
    }

    pub const fn fallback(self, fallback: FallbackFn<R>) -> FallbackField<R> {
        FallbackField {
            field: self,
            fallback,
        }
    }
}

impl<R> Field for RawField<R>
where
    Config: FieldReader<R>,
{
    type Output = Option<R>;

    fn name(&self) -> &'static str {
        self.name
    }

    fn get_from(&self, config: &Config) -> Self::Output {
        config.get_field(self.name).ok()
    }
}

type FallbackFn<R> = fn(&Config) -> R;

pub struct FallbackField<R> {
    field: RawField<R>,
    fallback: FallbackFn<R>,
}

impl<R> Field for FallbackField<R>
where
    Config: FieldReader<R>,
{
    type Output = R;

    fn name(&self) -> &'static str {
        self.field.name()
    }

    fn get_from(&self, config: &Config) -> Self::Output {
        self.field
            .get_from(config)
            .unwrap_or_else(|| (self.fallback)(config))
    }
}

pub trait ReflectField {
    fn name(&self) -> &'static str;

    fn dump(&self, config: &Config) -> String;
}

impl<F> ReflectField for F
where
    F: Field,
    F::Output: std::fmt::Display,
{
    fn name(&self) -> &'static str {
        self.name()
    }

    fn dump(&self, config: &Config) -> String {
        self.get_from(config).to_string()
    }
}
