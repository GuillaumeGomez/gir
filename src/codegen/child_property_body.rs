use analysis;
use chunk::Chunk;

#[derive(Default)]
pub struct Builder {
    name: String,
    rust_name: String,
    is_get: bool,
    default_value: String,
    type_string: String,
}

impl Builder {
    pub fn new() -> Builder {
        Default::default()
    }

    pub fn name(&mut self, name: &str) -> &mut Builder {
        self.name = name.into();
        self
    }

    pub fn rust_name(&mut self, name: &str) -> &mut Builder {
        self.rust_name = name.into();
        self
    }

    pub fn is_get(&mut self, value: bool) -> &mut Builder {
        self.is_get = value;
        self
    }

    pub fn default_value(&mut self, value: &str) -> &mut Builder {
        self.default_value = value.into();
        self
    }

    pub fn type_string(&mut self, type_: &str) -> &mut Builder {
        self.type_string = type_.into();
        self
    }

    pub fn generate(&self) -> Chunk {
        let chunks = if self.is_get { self.chunks_for_get() } else { self.chunks_for_set() };
        Chunk::BlockHalf(chunks)
    }

    fn chunks_for_get(&self) -> Vec<Chunk> {
        let mut params = Vec::new();

        params.push(Chunk::Custom("self.to_glib_none().0".into()));
        params.push(Chunk::Custom("item.to_glib_none().0".into()));
        params.push(Chunk::Custom(format!("\"{}\".to_glib_none().0", self.name)));
        params.push(Chunk::Custom("value.to_glib_none_mut().0".into()));

        let mut body = Vec::new();

        let return_info = analysis::return_value::Info {
            parameter: None,
            base_tid: None,
            commented: false,
        };
        let ffi_call = Chunk::FfiCall{
            name: "gtk_container_child_get_property".into(),
            params: params,
        };
        body.push(Chunk::FfiCallConversion{
            ret: return_info,
            call: Box::new(ffi_call),
        });

        let unsafe_ = Chunk::Unsafe(body);

        let mut chunks = Vec::new();

        let default_value_chunk = Chunk::Custom(format!("Value::from(&{})", self.default_value));
        chunks.push(Chunk::Let{
            name: "value".into(),
            is_mut: true,
            value: Box::new(default_value_chunk),
            type_: None
        });
        chunks.push(unsafe_);
        chunks.push(Chunk::Custom(format!("value.get::<{}>().unwrap()", self.type_string)));

        chunks
    }

    fn chunks_for_set(&self) -> Vec<Chunk> {
        let mut params = Vec::new();

        params.push(Chunk::Custom("self.to_glib_none().0".into()));
        params.push(Chunk::Custom("item.to_glib_none().0".into()));
        params.push(Chunk::Custom(format!("\"{}\".to_glib_none().0", self.name)));
        params.push(Chunk::Custom(format!("Value::from(&{}).to_glib_none().0", self.rust_name)));

        let mut body = Vec::new();

        let ffi_call = Chunk::FfiCall{
            name: "gtk_container_child_set_property".into(),
            params: params,
        };
        let return_info = analysis::return_value::Info {
            parameter: None,
            base_tid: None,
            commented: false,
        };
        body.push(Chunk::FfiCallConversion{
            ret: return_info,
            call: Box::new(ffi_call),
        });

        let unsafe_ = Chunk::Unsafe(body);

        let mut chunks = Vec::new();

        chunks.push(unsafe_);

        chunks
    }
}
