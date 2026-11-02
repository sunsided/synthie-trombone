// The original Pink Trombone's polar tract layout: cell index runs along an
// arc (glottis bottom left, over the palate, lips upper right), diameter
// runs inwards from the outer wall. Units are the original's canvas pixels.

export const ORIGIN = { x: 340, y: 449 };
export const RADIUS = 298;
export const SCALE = 60;
const ANGLE_SCALE = 0.64;
const ANGLE_OFFSET = -0.24;
/** Gap between the oral tract's outer wall and the nasal cavity. */
export const NOSE_OFFSET = 0.8;

/** Tongue control region, in the tract's own (index, diameter) units. */
export const TONGUE = {
	minIndex: 12,
	maxIndex: 29,
	minDiameter: 2.05,
	maxDiameter: 3.5
};

export interface Point {
	x: number;
	y: number;
}

/** Angle of a (fractional) cell index; `lipStart` fixes the arc's span. */
export function angle(index: number, lipStart: number): number {
	return ANGLE_OFFSET + (index * ANGLE_SCALE * Math.PI) / (lipStart - 1);
}

/** Point at `diameter` from the outer wall of cell `index`. */
export function point(index: number, diameter: number, lipStart: number): Point {
	const a = angle(index, lipStart);
	const r = RADIUS - SCALE * diameter;
	return { x: ORIGIN.x - r * Math.cos(a), y: ORIGIN.y - r * Math.sin(a) };
}

/** Inverse of `point`: the (index, diameter) under a canvas position. */
export function locate(p: Point, lipStart: number): { index: number; diameter: number } {
	const dx = p.x - ORIGIN.x;
	const dy = p.y - ORIGIN.y;
	let a = Math.atan2(dy, dx);
	while (a > 0) a -= 2 * Math.PI;
	const index = ((Math.PI + a - ANGLE_OFFSET) * (lipStart - 1)) / (ANGLE_SCALE * Math.PI);
	const diameter = (RADIUS - Math.hypot(dx, dy)) / SCALE;
	return { index, diameter };
}

/** Whether a pointer at (index, diameter) grabs the tongue body. */
export function onTongue(index: number, diameter: number): boolean {
	return (
		index >= TONGUE.minIndex - 4 &&
		index <= TONGUE.maxIndex + 4 &&
		diameter >= TONGUE.minDiameter - 0.5 &&
		diameter <= TONGUE.maxDiameter + 0.5
	);
}

/** Bounding box of the drawing, in original units. */
export const VIEW = { x: -175, y: -75, width: 760, height: 650 };
