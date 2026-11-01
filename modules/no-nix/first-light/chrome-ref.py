#!/usr/bin/env python3
# Lifts COMMON and MAIN verbatim out of the approved ring.html and wraps them in a
# page with a canvas of exactly 3200x2000, so Chrome renders the approved shader at
# the panel's real resolution. Nothing of the shader is retyped.
import re,sys
src=open(sys.argv[1]).read()
common=re.search(r'const COMMON = `(.*?)`;',src,re.S).group(1)
main=re.search(r'const MAIN = `(.*?)`;',src,re.S).group(1)
html='''<!doctype html><html><head><style>html,body{margin:0;background:#000}canvas{display:block;width:3200px;height:2000px}</style></head><body><canvas id=c width=3200 height=2000></canvas><script>
const COMMON=%s;const MAIN=%s;
const sm=(a,b,x)=>{const t=Math.min(1,Math.max(0,(x-a)/(b-a)));return t*t*(3-2*t)};
const t=parseFloat(new URLSearchParams(location.search).get('t'));
const gl=document.getElementById('c').getContext('webgl2',{antialias:false,alpha:false,preserveDrawingBuffer:true});
function sh(ty,s){const o=gl.createShader(ty);gl.shaderSource(o,s);gl.compileShader(o);if(!gl.getShaderParameter(o,gl.COMPILE_STATUS))document.title='ERR '+gl.getShaderInfoLog(o);return o}
const p=gl.createProgram();gl.attachShader(p,sh(gl.VERTEX_SHADER,'attribute vec2 a; void main(){ gl_Position = vec4(a, 0., 1.); }'));gl.attachShader(p,sh(gl.FRAGMENT_SHADER,COMMON+MAIN));gl.linkProgram(p);gl.useProgram(p);
gl.bindBuffer(gl.ARRAY_BUFFER,gl.createBuffer());gl.bufferData(gl.ARRAY_BUFFER,new Float32Array([-1,-1,3,-1,-1,3]),gl.STATIC_DRAW);const a=gl.getAttribLocation(p,'a');gl.enableVertexAttribArray(a);gl.vertexAttribPointer(a,2,gl.FLOAT,false,0,0);
gl.viewport(0,0,3200,2000);gl.uniform2f(gl.getUniformLocation(p,'R'),3200,2000);gl.uniform1f(gl.getUniformLocation(p,'T'),t);gl.uniform1f(gl.getUniformLocation(p,'EXIT'),sm(11.6,13.4,t));gl.drawArrays(gl.TRIANGLES,0,3);
</script></body></html>'''%(repr(common).replace('\\n','\\n'),repr(main))
open('ref.html','w').write(html)
