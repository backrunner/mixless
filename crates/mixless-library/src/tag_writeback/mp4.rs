use lofty::{
    mp4::{Atom, AtomData, AtomIdent, DataType, Ilst},
    tag::Tag,
};

const TEMPO: AtomIdent<'static> = AtomIdent::Fourcc(*b"tmpo");

pub(super) fn with_tempo(tag: Tag, bpm: Option<f32>) -> (Ilst, bool) {
    let mut ilst = Ilst::from(tag);
    let Some(bpm) = bpm
        .filter(|b| b.is_finite())
        .map(f32::round)
        .filter(|b| (1.0..=u16::MAX as f32).contains(b))
        .map(|b| b as u16)
    else {
        return (ilst, false);
    };
    let unique = (&ilst)
        .into_iter()
        .filter(|atom| atom.ident() == &TEMPO)
        .count()
        == 1;
    let matches = unique
        && ilst.get(&TEMPO).is_some_and(|atom| {
            let mut values = atom.data();
            let matches = match values.next() {
                Some(AtomData::SignedInteger(value)) => *value == i32::from(bpm),
                Some(AtomData::UnsignedInteger(value)) => *value == u32::from(bpm),
                _ => false,
            };
            matches && values.next().is_none()
        });
    // iTunes tmpo is a two-byte big-endian integer with data type 21.
    // Lofty's integer variants shrink the payload, so preserve its width.
    ilst.replace_atom(Atom::new(
        TEMPO,
        AtomData::Unknown {
            code: DataType::BeSignedInteger,
            data: bpm.to_be_bytes().to_vec(),
        },
    ));
    (ilst, !matches)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lofty::{
        config::WriteOptions,
        tag::{ItemKey, TagExt, TagType},
    };

    #[test]
    fn tempo_is_serialized_as_a_two_byte_integer_and_keeps_other_atoms() {
        for bpm in [75., 149.99, 255., 256.] {
            let mut original = Ilst::new();
            original.insert(Atom::new(TEMPO, AtomData::UTF8(format!("{bpm:.0}"))));
            let opaque = Atom::new(
                AtomIdent::Fourcc(*b"test"),
                AtomData::Unknown {
                    code: DataType::Reserved,
                    data: vec![1, 2, 3, 4],
                },
            );
            original.insert(opaque.clone());
            let mut tag = Tag::from(original);
            tag.insert_text(ItemKey::Bpm, format!("{bpm:.2}"));
            tag.insert_text(ItemKey::InitialKey, "Am".into());
            let (fixed, changed) = with_tempo(tag, Some(bpm));
            assert!(changed);
            assert_eq!(fixed.get(&AtomIdent::Fourcc(*b"test")), Some(&opaque));
            let mut bytes = Vec::new();
            fixed.dump_to(&mut bytes, WriteOptions::default()).unwrap();
            let start = bytes.windows(4).position(|b| b == b"tmpo").unwrap();
            let expected: Vec<_> = [
                18_u32.to_be_bytes().as_slice(),
                b"data",
                21_u32.to_be_bytes().as_slice(),
                &[0, 0, 0, 0],
                &(bpm.round() as u16).to_be_bytes(),
            ]
            .concat();
            assert_eq!(&bytes[start + 4..start + 22], expected);
            let generic = Tag::from(fixed);
            assert_eq!(
                generic.get_string(ItemKey::Bpm),
                Some(format!("{bpm:.2}").as_str())
            );
            assert_eq!(generic.get_string(ItemKey::InitialKey), Some("Am"));
        }
    }

    #[test]
    fn existing_integer_is_idempotent_and_missing_bpm_does_not_delete_tags() {
        let mut ilst = Ilst::new();
        ilst.insert(Atom::new(TEMPO, AtomData::SignedInteger(150)));
        assert!(!with_tempo(Tag::from(ilst.clone()), Some(149.99)).1);
        for bpm in [None, Some(f32::NAN), Some(-1.), Some(0.)] {
            let (preserved, changed) = with_tempo(Tag::from(ilst.clone()), bpm);
            assert!(!changed);
            assert_eq!(preserved.get(&TEMPO), ilst.get(&TEMPO));
        }
        assert!(with_tempo(Tag::new(TagType::Mp4Ilst), Some(150.)).1);
    }

    #[test]
    fn a_native_integer_cannot_hide_a_duplicate_legacy_text_tempo() {
        let mut ilst = Ilst::new();
        ilst.insert(Atom::new(TEMPO, AtomData::SignedInteger(150)));
        let mut tag = Tag::from(ilst);
        // Generic writeback used to retain the native companion atom while
        // also appending a text tmpo. The first atom alone looks correct.
        tag.insert_text(ItemKey::IntegerBpm, "150".into());
        assert_eq!(
            (&Ilst::from(tag.clone()))
                .into_iter()
                .filter(|a| a.ident() == &TEMPO)
                .count(),
            2
        );
        let (fixed, repaired) = with_tempo(tag, Some(150.));
        assert!(repaired);
        assert_eq!(
            (&fixed).into_iter().filter(|a| a.ident() == &TEMPO).count(),
            1
        );
        assert_eq!(fixed.get(&TEMPO).unwrap().data().count(), 1);
    }
}
