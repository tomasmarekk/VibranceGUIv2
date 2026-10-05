// Color-rule pass for the screen overlay. Reads the captured desktop and writes the
// recolored pixel with premultiplied alpha; unmatched pixels stay fully transparent.
// Mirrors color_match.rs (CPU reference) and src/colorMatch.ts (preview).

static const uint MAX_RULES = 4;
static const float LIGHTNESS_WEIGHT = 0.15;
static const float MIN_LIGHTNESS = 0.05;
static const float MAX_SHADE = 1.5;

cbuffer Rules : register(b0)
{
    float4 ruleSource[MAX_RULES];   // xyz: OKLab color to find
    float4 ruleTarget[MAX_RULES];   // xyz: OKLab color matched pixels take on
    float4 ruleParams[MAX_RULES];   // x: radius, y: strength, z: chroma scale, w: lightness offset
    uint4 ruleCount;                // x: number of rules in use
};

Texture2D<float4> desktop : register(t0);

float4 VSMain(uint id : SV_VertexID) : SV_Position
{
    // One triangle that covers the whole viewport.
    float2 corner = float2((id << 1) & 2, id & 2);
    return float4(corner * float2(2.0, -2.0) + float2(-1.0, 1.0), 0.0, 1.0);
}

float3 SrgbToLinear(float3 c)
{
    return lerp(c / 12.92, pow((c + 0.055) / 1.055, 2.4), step(0.04045, c));
}

float3 LinearToSrgb(float3 c)
{
    return lerp(c * 12.92, 1.055 * pow(c, 1.0 / 2.4) - 0.055, step(0.0031308, c));
}

float3 LinearToOklab(float3 c)
{
    float3 lms = float3(
        0.4122214708 * c.r + 0.5363325363 * c.g + 0.0514459929 * c.b,
        0.2119034982 * c.r + 0.6806995451 * c.g + 0.1073969566 * c.b,
        0.0883024619 * c.r + 0.2817188376 * c.g + 0.6299787005 * c.b);
    lms = pow(max(lms, 0.0), 1.0 / 3.0);
    return float3(
        0.2104542553 * lms.x + 0.7936177850 * lms.y - 0.0040720468 * lms.z,
        1.9779984951 * lms.x - 2.4285922050 * lms.y + 0.4505937099 * lms.z,
        0.0259040371 * lms.x + 0.7827717662 * lms.y - 0.8086757660 * lms.z);
}

float3 OklabToLinear(float3 lab)
{
    float3 lms = float3(
        lab.x + 0.3963377774 * lab.y + 0.2158037573 * lab.z,
        lab.x - 0.1055613458 * lab.y - 0.0638541728 * lab.z,
        lab.x - 0.0894841775 * lab.y - 1.2914855480 * lab.z);
    lms = lms * lms * lms;
    return float3(
        4.0767416621 * lms.x - 3.3077115913 * lms.y + 0.2309699292 * lms.z,
        -1.2684380046 * lms.x + 2.6097574011 * lms.y - 0.3413193965 * lms.z,
        -0.0041960863 * lms.x - 0.7034186147 * lms.y + 1.7076147010 * lms.z);
}

float4 PSMain(float4 position : SV_Position) : SV_Target
{
    float3 lab = LinearToOklab(SrgbToLinear(desktop.Load(int3(position.xy, 0)).rgb));
    float best = 0.0;
    float opacity = 0.0;
    float3 adjusted = lab;
    for (uint i = 0; i < ruleCount.x; i++)
    {
        // Chromaticity (a/L, b/L) ignores shading, so darker edges of an outline match.
        float3 source = ruleSource[i].xyz;
        float sourceLightness = max(source.x, MIN_LIGHTNESS);
        float2 chroma = (lab.yz / max(lab.x, MIN_LIGHTNESS) - source.yz / sourceLightness) * sourceLightness;
        float lightness = (lab.x - source.x) * LIGHTNESS_WEIGHT;
        float radius = ruleParams[i].x;
        float weight = 1.0 - smoothstep(radius * 0.5, radius, length(float3(lightness, chroma)));
        if (weight > best)
        {
            best = weight;
            float shade = clamp(lab.x / sourceLightness, 0.0, MAX_SHADE);
            float3 shaded = ruleTarget[i].xyz * shade;
            adjusted = float3(shaded.x + ruleParams[i].w, shaded.yz * ruleParams[i].z);
            opacity = weight * ruleParams[i].y;
        }
    }
    if (opacity <= 0.0)
    {
        return float4(0.0, 0.0, 0.0, 0.0);
    }
    float3 color = LinearToSrgb(saturate(OklabToLinear(adjusted)));
    return float4(color * opacity, opacity);
}
