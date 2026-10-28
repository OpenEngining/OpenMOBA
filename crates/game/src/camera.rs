use glam::{Mat4, Vec2, Vec3, Vec4Swizzles};

pub const PITCH: f32 = 56.0_f32.to_radians();
pub const FIELD_OF_VIEW: f32 = 40.0_f32.to_radians();
pub const MIN_DISTANCE: f32 = 1150.0;
pub const MAX_DISTANCE: f32 = 1950.0;
pub const ZOOM_STEP: f32 = 0.12;
pub const ZOOM_SMOOTHING: f32 = 12.0;

const LEFT_HANDED: Mat4 = Mat4::from_cols_array(&[
    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
]);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    pub focus: Vec2,
    pub height: f32,
    pub distance: f32,
    pub target_distance: f32,
    pub locked: bool,
}

impl Camera {
    pub fn new(focus: Vec2) -> Self {
        Self {
            focus,
            height: 0.0,
            distance: MAX_DISTANCE,
            target_distance: MAX_DISTANCE,
            locked: false,
        }
    }

    pub fn zoom(&mut self, steps: f32) {
        self.target_distance =
            (self.target_distance * (1.0 + ZOOM_STEP * steps)).clamp(MIN_DISTANCE, MAX_DISTANCE);
    }

    pub fn update(&mut self, dt: f32) {
        let blend = 1.0 - (-ZOOM_SMOOTHING * dt).exp();
        self.distance += (self.target_distance - self.distance) * blend;
    }

    pub fn eye(&self) -> Vec3 {
        Vec3::new(
            self.focus.x,
            self.height + self.distance * PITCH.sin(),
            self.focus.y - self.distance * PITCH.cos(),
        )
    }

    pub fn view_projection(&self, aspect: f32) -> Mat4 {
        let focus = Vec3::new(self.focus.x, self.height, self.focus.y);
        let projection = Mat4::perspective_rh_gl(FIELD_OF_VIEW, aspect, 10.0, 20000.0);
        let view = Mat4::look_at_rh(
            LEFT_HANDED.transform_point3(self.eye()),
            LEFT_HANDED.transform_point3(focus),
            Vec3::Y,
        );
        projection * view * LEFT_HANDED
    }

    pub fn point_at_height(&self, ndc: Vec2, aspect: f32, height: f32) -> Option<Vec2> {
        let inverse = self.view_projection(aspect).inverse();
        let unproject = |z: f32| {
            let point = inverse * ndc.extend(z).extend(1.0);
            point.xyz() / point.w
        };
        let near = unproject(-1.0);
        let direction = unproject(1.0) - near;
        if direction.y >= 0.0 {
            return None;
        }
        let hit = near - direction * ((near.y - height) / direction.y);
        Some(Vec2::new(hit.x, hit.z))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_center_hits_the_focus() {
        let camera = Camera::new(Vec2::new(300.0, 700.0));
        let hit = camera
            .point_at_height(Vec2::ZERO, 16.0 / 9.0, camera.height)
            .unwrap();
        assert!((hit - camera.focus).length() < 0.5);
    }

    #[test]
    fn screen_right_is_plus_x_and_screen_up_is_plus_z() {
        let camera = Camera::new(Vec2::ZERO);
        let right = camera
            .point_at_height(Vec2::new(0.5, 0.0), 1.0, camera.height)
            .unwrap();
        let up = camera
            .point_at_height(Vec2::new(0.0, 0.5), 1.0, camera.height)
            .unwrap();
        assert!(right.x > 100.0 && right.y.abs() < 1.0);
        assert!(up.y > 100.0 && up.x.abs() < 1.0);
    }

    #[test]
    fn ground_point_round_trips_through_projection() {
        let camera = Camera::new(Vec2::new(-200.0, 50.0));
        let world = Vec3::new(150.0, 0.0, 400.0);
        let clip = camera.view_projection(1.5) * world.extend(1.0);
        let ndc = clip.xy() / clip.w;
        let hit = camera.point_at_height(ndc, 1.5, camera.height).unwrap();
        assert!((hit - Vec2::new(150.0, 400.0)).length() < 0.5);
    }

    #[test]
    fn raised_ground_still_centers_the_focus() {
        let mut camera = Camera::new(Vec2::new(300.0, 700.0));
        camera.height = -180.0;
        let hit = camera
            .point_at_height(Vec2::ZERO, 16.0 / 9.0, camera.height)
            .unwrap();
        assert!((hit - camera.focus).length() < 0.5);
        let world = Vec3::new(150.0, -100.0, 900.0);
        let clip = camera.view_projection(1.5) * world.extend(1.0);
        let hit = camera
            .point_at_height(clip.xy() / clip.w, 1.5, -100.0)
            .unwrap();
        assert!((hit - Vec2::new(150.0, 900.0)).length() < 0.5);
    }

    #[test]
    fn zoom_stays_in_range() {
        let mut camera = Camera::new(Vec2::ZERO);
        camera.zoom(100.0);
        assert_eq!(camera.target_distance, MAX_DISTANCE);
        camera.zoom(-100.0);
        assert_eq!(camera.target_distance, MIN_DISTANCE);
    }
}
