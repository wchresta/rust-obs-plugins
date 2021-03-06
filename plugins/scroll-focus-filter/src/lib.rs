mod server;

use server::{Server, WindowSnapshot};

use obs_wrapper::{graphics::*, obs_register_module, obs_string, prelude::*, source::*};

use crossbeam_channel::{unbounded, Receiver, Sender};

enum FilterMessage {
    CloseConnection,
}

enum ServerMessage {
    Snapshot(WindowSnapshot),
}

struct Data {
    source: SourceContext,
    effect: GraphicsEffect,

    mul_val: GraphicsEffectVec2Param,
    add_val: GraphicsEffectVec2Param,
    image: GraphicsEffectTextureParam,

    sampler: GraphicsSamplerState,

    send: Sender<FilterMessage>,
    receive: Receiver<ServerMessage>,

    current: Vec2,
    from: Vec2,
    target: Vec2,

    animation_time: f64,

    current_zoom: f64,
    from_zoom: f64,
    target_zoom: f64,
    internal_zoom: f64,
    padding: f64,

    progress: f64,

    screen_width: u32,
    screen_height: u32,
    screen_x: u32,
    screen_y: u32,
}

impl Drop for Data {
    fn drop(&mut self) {
        self.send.send(FilterMessage::CloseConnection).unwrap_or(());
    }
}

struct ScrollFocusFilter {
    context: ModuleContext,
}

impl Sourceable for ScrollFocusFilter {
    fn get_id() -> ObsString {
        obs_string!("scroll_focus_filter")
    }
    fn get_type() -> SourceType {
        SourceType::FILTER
    }
}

impl GetNameSource<Data> for ScrollFocusFilter {
    fn get_name() -> ObsString {
        obs_string!("Scroll Focus Filter")
    }
}

impl GetPropertiesSource<Data> for ScrollFocusFilter {
    fn get_properties(_data: &mut Option<Data>, properties: &mut Properties) {
        properties.add_float_slider(
            obs_string!("zoom"),
            obs_string!("Amount to zoom in window"),
            1.,
            5.,
            0.001,
        );
        properties.add_int(
            obs_string!("screen_x"),
            obs_string!("Offset relative to top left screen - x"),
            0,
            3840 * 3,
            1,
        );
        properties.add_int(
            obs_string!("screen_y"),
            obs_string!("Offset relative to top left screen - y"),
            0,
            3840 * 3,
            1,
        );
        properties.add_float_slider(
            obs_string!("padding"),
            obs_string!("Padding around each window"),
            0.,
            0.5,
            0.001,
        );
        properties.add_int(
            obs_string!("screen_width"),
            obs_string!("Screen width"),
            1,
            3840 * 3,
            1,
        );
        properties.add_int(
            obs_string!("screen_height"),
            obs_string!("Screen height"),
            1,
            3840 * 3,
            1,
        );
        properties.add_float(
            obs_string!("animation_time"),
            obs_string!("Animation Time (s)"),
            0.3,
            10.,
            0.001,
        );
    }
}

fn smooth_step(x: f32) -> f32 {
    let t = ((x / 1.).max(0.)).min(1.);
    t * t * (3. - 2. * t)
}

impl VideoTickSource<Data> for ScrollFocusFilter {
    fn video_tick(data: &mut Option<Data>, seconds: f32) {
        if let Some(data) = data {
            for message in data.receive.try_iter() {
                match message {
                    ServerMessage::Snapshot(snapshot) => {
                        let window_zoom = ((snapshot.width / (data.screen_width as f32))
                            .max(snapshot.height / (data.screen_height as f32))
                            as f64
                            + data.padding)
                            .max(data.internal_zoom)
                            .min(1.);

                        if snapshot.x > (data.screen_width + data.screen_x) as f32
                            || snapshot.x < data.screen_x as f32
                            || snapshot.y < data.screen_y as f32
                            || snapshot.y > (data.screen_height + data.screen_y) as f32
                        {
                            if data.target_zoom != 1.
                                && data.target.x() != 0.
                                && data.target.y() != 0.
                            {
                                data.progress = 0.;
                                data.from_zoom = data.current_zoom;
                                data.target_zoom = 1.;

                                data.from.set(data.current.x(), data.current.y());
                                data.target.set(0., 0.);
                            }
                        } else {
                            let x = (snapshot.x + (snapshot.width / 2.) - (data.screen_x as f32))
                                / (data.screen_width as f32);
                            let y = (snapshot.y + (snapshot.height / 2.) - (data.screen_y as f32))
                                / (data.screen_height as f32);

                            let target_x = (x - (0.5 * window_zoom as f32))
                                .min(1. - window_zoom as f32)
                                .max(0.);

                            let target_y = (y - (0.5 * window_zoom as f32))
                                .min(1. - window_zoom as f32)
                                .max(0.);

                            if (target_y - data.target.y()).abs() > 0.001
                                || (target_x - data.target.x()).abs() > 0.001
                                || (window_zoom - data.target_zoom).abs() > 0.001
                            {
                                data.progress = 0.;

                                data.from_zoom = data.current_zoom;
                                data.target_zoom = window_zoom;

                                data.from.set(data.current.x(), data.current.y());

                                data.target.set(target_x, target_y);
                            }
                        }
                    }
                }
            }

            data.progress = (data.progress + seconds as f64 / data.animation_time).min(1.);

            let adjusted_progress = smooth_step(data.progress as f32);

            data.current.set(
                data.from.x() + (data.target.x() - data.from.x()) * adjusted_progress,
                data.from.y() + (data.target.y() - data.from.y()) * adjusted_progress,
            );

            data.current_zoom =
                data.from_zoom + (data.target_zoom - data.from_zoom) * adjusted_progress as f64;
        }
    }
}

impl VideoRenderSource<Data> for ScrollFocusFilter {
    fn video_render(
        data: &mut Option<Data>,
        _context: &mut GlobalContext,
        render: &mut VideoRenderContext,
    ) {
        if let Some(data) = data {
            let effect = &mut data.effect;
            let source = &mut data.source;
            let param_add = &mut data.add_val;
            let param_mul = &mut data.mul_val;
            let image = &mut data.image;
            let sampler = &mut data.sampler;

            let current = &mut data.current;

            let zoom = data.current_zoom as f32;

            let mut cx: u32 = 1;
            let mut cy: u32 = 1;

            source.do_with_target(|target| {
                cx = target.get_base_width();
                cy = target.get_base_height();
            });

            source.process_filter(
                render,
                effect,
                (cx, cy),
                GraphicsColorFormat::RGBA,
                GraphicsAllowDirectRendering::NoDirectRendering,
                |context, _effect| {
                    param_add.set_vec2(context, &Vec2::new(current.x(), current.y()));
                    param_mul.set_vec2(context, &Vec2::new(zoom, zoom));
                    image.set_next_sampler(context, sampler);
                },
            );
        }
    }
}

impl CreatableSource<Data> for ScrollFocusFilter {
    fn create(
        settings: &mut SettingsContext,
        mut source: SourceContext,
        _context: &mut GlobalContext,
    ) -> Data {
        if let Some(mut effect) = GraphicsEffect::from_effect_string(
            obs_string!(include_str!("./crop_filter.effect")),
            obs_string!("crop_filter.effect"),
        ) {
            if let Some(image) = effect.get_effect_param_by_name(obs_string!("image")) {
                if let Some(add_val) = effect.get_effect_param_by_name(obs_string!("add_val")) {
                    if let Some(mul_val) = effect.get_effect_param_by_name(obs_string!("mul_val")) {
                        let zoom = 1. / settings.get_float(obs_string!("zoom")).unwrap_or(1.);

                        let sampler = GraphicsSamplerState::from(GraphicsSamplerInfo::default());

                        let screen_width = settings
                            .get_int(obs_string!("screen_width"))
                            .unwrap_or(1920) as u32;
                        let screen_height = settings
                            .get_int(obs_string!("screen_height"))
                            .unwrap_or(1080) as u32;

                        let screen_x =
                            settings.get_int(obs_string!("screen_x")).unwrap_or(0) as u32;
                        let screen_y =
                            settings.get_int(obs_string!("screen_y")).unwrap_or(0) as u32;

                        let animation_time = settings
                            .get_float(obs_string!("animation_time"))
                            .unwrap_or(0.3);

                        let (send_filter, receive_filter) = unbounded::<FilterMessage>();
                        let (send_server, receive_server) = unbounded::<ServerMessage>();

                        std::thread::spawn(move || {
                            let mut server = Server::new().unwrap();

                            loop {
                                if let Some(snapshot) = server.wait_for_event() {
                                    send_server
                                        .send(ServerMessage::Snapshot(snapshot))
                                        .unwrap_or(());
                                }

                                if let Ok(msg) = receive_filter.try_recv() {
                                    match msg {
                                        FilterMessage::CloseConnection => {
                                            return;
                                        }
                                    }
                                }
                            }
                        });

                        source.update_source_settings(settings);

                        return Data {
                            source,
                            effect,
                            add_val,
                            mul_val,
                            image,

                            sampler,

                            animation_time,

                            current_zoom: zoom,
                            from_zoom: zoom,
                            target_zoom: zoom,
                            internal_zoom: zoom,

                            send: send_filter,
                            receive: receive_server,

                            current: Vec2::new(0., 0.),
                            from: Vec2::new(0., 0.),
                            target: Vec2::new(0., 0.),
                            padding: 0.1,

                            progress: 1.,

                            screen_height,
                            screen_width,
                            screen_x,
                            screen_y,
                        };
                    }
                }
            }

            panic!("Failed to find correct effect params!");
        } else {
            panic!("Could not load crop filter effect!");
        }
    }
}

impl UpdateSource<Data> for ScrollFocusFilter {
    fn update(
        data: &mut Option<Data>,
        settings: &mut SettingsContext,
        _context: &mut GlobalContext,
    ) {
        if let Some(data) = data {
            if let Some(zoom) = settings.get_float(obs_string!("zoom")) {
                data.from_zoom = data.current_zoom;
                data.internal_zoom = 1. / zoom;
                data.target_zoom = 1. / zoom;
            }

            if let Some(screen_width) = settings.get_int(obs_string!("screen_width")) {
                data.screen_width = screen_width as u32;
            }

            if let Some(padding) = settings.get_float(obs_string!("padding")) {
                data.padding = padding;
            }

            if let Some(animation_time) = settings.get_float(obs_string!("animation_time")) {
                data.animation_time = animation_time;
            }

            if let Some(screen_height) = settings.get_int(obs_string!("screen_height")) {
                data.screen_height = screen_height as u32;
            }
            if let Some(screen_x) = settings.get_int(obs_string!("screen_x")) {
                data.screen_x = screen_x as u32;
            }
            if let Some(screen_y) = settings.get_int(obs_string!("screen_y")) {
                data.screen_y = screen_y as u32;
            }
        }
    }
}

impl Module for ScrollFocusFilter {
    fn new(context: ModuleContext) -> Self {
        Self { context }
    }
    fn get_ctx(&self) -> &ModuleContext {
        &self.context
    }

    fn load(&mut self, load_context: &mut LoadContext) -> bool {
        let source = load_context
            .create_source_builder::<ScrollFocusFilter, Data>()
            .enable_get_name()
            .enable_create()
            .enable_get_properties()
            .enable_update()
            .enable_video_render()
            .enable_video_tick()
            .build();

        load_context.register_source(source);

        true
    }

    fn description() -> ObsString {
        obs_string!("A filter that focused the currently focused Xorg window.")
    }
    fn name() -> ObsString {
        obs_string!("Scroll Focus Filter")
    }
    fn author() -> ObsString {
        obs_string!("Bennett Hardwick")
    }
}

obs_register_module!(ScrollFocusFilter);
