define_component(#{
 metadata: #{id:"components/watch_timer", "export":"WatchTimer", version:"0.1.0", runtime_api:#{min_inclusive:2,max_exclusive:3},dependencies:[],capabilities:#{},assets:[]},
 schema: #{
  props: #{
   key: #{schema:#{type:"string"},required:true,sensitive:false},
   sequence: #{schema:#{type:"integer",min:0},required:true,sensitive:false},
   on_tick: #{schema:#{type:"callback"},required:true,sensitive:false}
  },
  state:#{fields:#{}},events:#{tick:#{payload:#{type:"null"}}},slots:#{},parts:["root"]
 },
 render:Fn("render_timer")
});
fn fired(ctx,payload) {ctx.emit("tick",());}
fn WatchTimer(props) {render_component("components/watch_timer",props)}
fn render_timer(ctx,props) {
 timeout(`watch-${props.sequence}`,750,false,Fn("fired"),());
 row([])
}
