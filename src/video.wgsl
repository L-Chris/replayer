@group(0) @binding(0) var y_plane: texture_2d<f32>;
@group(0) @binding(1) var u_plane: texture_2d<f32>;
@group(0) @binding(2) var v_plane: texture_2d<f32>;
@group(0) @binding(3) var linear_sampler: sampler;
struct Matrix { r: vec4<f32>, g: vec4<f32>, b: vec4<f32>, flags: vec4<f32> }
@group(0) @binding(4) var<uniform> matrix: Matrix;
struct Vertex { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> }
@vertex fn vs_main(@builtin(vertex_index) id: u32) -> Vertex {
    let points = array<vec2<f32>,3>(vec2(-1.0,3.0),vec2(-1.0,-1.0),vec2(3.0,-1.0));
    var out: Vertex;
    out.pos = vec4(points[id],0.0,1.0);
    out.uv = vec2((points[id].x+1.0)*0.5,(1.0-points[id].y)*0.5);
    return out;
}
@fragment fn fs_main(in: Vertex) -> @location(0) vec4<f32> {
    let y = textureSample(y_plane,linear_sampler,in.uv).r;
    let uv = textureSample(u_plane,linear_sampler,in.uv).rg;
    let v = select(textureSample(v_plane,linear_sampler,in.uv).r,uv.g,matrix.flags.x > 0.5);
    let sample = vec4(y,uv.r,v,1.0);
    return vec4(clamp(vec3(dot(matrix.r,sample),dot(matrix.g,sample),dot(matrix.b,sample)),vec3(0.0),vec3(1.0)),1.0);
}
